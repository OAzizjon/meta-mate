//! "Where's the button?" (docs/plan.md Day 4), the part between the platform and the
//! model: one look at the target window turned into what the model may see and point at,
//! and its answer turned back into a place on the screen.
//!
//! The look is a UIA snapshot and a capture of the same region (the window plus its owned
//! popups). Redaction is the only way to an image (invariant 3: a truncated snapshot or a
//! stale one fails closed); the element list names exactly the elements the model may
//! point at; a point resolves to an element's rectangle or to nothing (invariant 5).

use mm_core::api::request::{ElementLine, ElementList, must_hide, render_element_list};
use mm_core::geom::{PhysRect, ShotPoint};
use mm_core::matescript::{Mode, Point, Settings};
use mm_core::redaction::{
    Field, Redacted, RedactionError, RedactionInput, overlaps_any, painted_areas, point_in_any,
    redact,
};
use mm_core::shot::Bgra;
use mm_platform::{CapturedImage, UiElement, UiSnapshot};

/// The long edge of the image the model gets (S4 measured ~3.7k tokens at 1280 px).
pub const MAX_EDGE: u32 = 1280;
/// Elements listed in the prompt: interactive ones first (render_element_list).
pub const MAX_LISTED: usize = 60;

/// One look at the target, ready for a point request.
pub struct Look {
    pub snapshot: UiSnapshot,
    /// The walk was cut and a separate sensitive-field scan vouched for the picture.
    pub cut: bool,
    pub redacted: Redacted,
    pub list: ElementList,
    /// The target window's own frame when it was captured: pointing later checks the
    /// window is still there, unmoved (`take_look_at` sets it; `prepare` uses the region).
    pub window_frame: PhysRect,
    /// What was painted over besides password fields: the parts of the region that are
    /// not the target app (gaps beside its popups, other apps' windows above it). Nothing
    /// there is listed or pointed at.
    pub painted: Vec<PhysRect>,
}

impl std::fmt::Debug for Look {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Look")
            .field("elements", &self.snapshot.elements.len())
            .field("listed", &self.list.ids.len())
            .field("painted", &self.redacted.painted)
            .finish()
    }
}

#[derive(Debug)]
pub enum LookError {
    /// Redaction refused (truncated or stale snapshot, a field it cannot place, ...).
    Redaction(RedactionError),
    /// The capture does not have the shape of a BGRA image.
    Capture,
    /// A field that must be hidden is on screen but UIA gave no bounds for it: it cannot
    /// be painted, so nothing is sent (invariant 3, fail closed).
    UnlocatedSensitive,
}

impl std::fmt::Display for LookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Redaction(error) => write!(f, "redaction refused: {error}"),
            Self::Capture => f.write_str("the capture is not a BGRA image"),
            Self::UnlocatedSensitive => f.write_str("a field to hide has no bounds"),
        }
    }
}

impl std::error::Error for LookError {}

/// Redacts `capture` (taken of `snapshot.frame`) and lists the elements the model may
/// point at. Every element UIA reports as a password, that looks like one (D37) or sits
/// beside a password's reveal toggle (`hidden_ids`) is painted over and its name never
/// reaches the prompt; so is every area in `painted` (not the target app).
pub fn prepare(
    snapshot: UiSnapshot,
    capture: &CapturedImage,
    painted: &[PhysRect],
) -> Result<Look, LookError> {
    let hidden = snapshot.hidden_ids();
    let must_paint = |element: &mm_platform::UiElement| {
        hidden.contains(&element.id) || must_hide(element.is_password, element.password_like, false)
    };
    // UIA reports an unrendered element without bounds (and offscreen); one it calls on
    // screen without bounds could be anywhere in the picture.
    let unlocated = snapshot
        .elements
        .iter()
        .any(|element| element.rect.is_none() && !element.offscreen && must_paint(element));
    if unlocated {
        return Err(LookError::UnlocatedSensitive);
    }
    let fields: Vec<Field> = snapshot
        .elements
        .iter()
        .filter_map(|element| {
            Some(Field {
                rect: element.rect?,
                is_password: element.is_password || hidden.contains(&element.id),
                password_like: element.password_like,
                was_password_this_session: false,
            })
        })
        .chain(painted.iter().map(|&rect| Field {
            rect,
            is_password: true,
            password_like: false,
            was_password_this_session: false,
        }))
        .collect();
    let bgra = Bgra::new(capture.width(), capture.height(), capture.pixels())
        .map_err(|_| LookError::Capture)?;
    let redacted = redact(&RedactionInput {
        capture: bgra,
        region: snapshot.frame,
        snapshot_region: snapshot.frame,
        fields: &fields,
        snapshot_truncated: snapshot.truncated,
        unclassified_sensitive_field: false,
        max_edge: MAX_EDGE,
    })
    .map_err(LookError::Redaction)?;
    let lines: Vec<ElementLine> = snapshot
        .elements
        .iter()
        .filter(|element| !element.offscreen && !element.id.starts_with(SEARCHED))
        .filter_map(|element| {
            let rect = element.rect?;
            // Under another app's window: the model cannot see it, so it cannot be meant.
            if overlaps_any(rect, painted) {
                return None;
            }
            ElementLine::new(
                &element.id,
                &element.control_type,
                &element.name,
                rect,
                redacted.frame,
                element.interactive,
                must_paint(element),
            )
        })
        .collect();
    let list = render_element_list(&lines, MAX_LISTED);
    let window_frame = snapshot.frame;
    Ok(Look {
        snapshot,
        cut: false,
        redacted,
        list,
        window_frame,
        painted: painted.to_vec(),
    })
}

impl Look {
    /// Validate a live hit without trusting its snapshot-local id or unknown ancestry.
    pub fn resolve_live(&self, element: Option<UiElement>) -> Option<PhysRect> {
        let element = element?;
        if !element.interactive
            || !element.enabled
            || element.offscreen
            || element.is_password
            || element.password_like
        {
            return None;
        }
        let rect = element.rect?;
        let frame = self.snapshot.frame;
        if rect.left >= rect.right
            || rect.top >= rect.bottom
            || clip(rect, frame) != Some(rect)
            || overlaps_any(rect, &self.painted)
        {
            return None;
        }
        let area = |r: PhysRect| {
            (i128::from(r.right) - i128::from(r.left)) * (i128::from(r.bottom) - i128::from(r.top))
        };
        if area(rect) * 5 > area(frame) * 2 {
            return None;
        }
        let hidden = self.snapshot.hidden_ids();
        if self.snapshot.elements.iter().any(|element| {
            hidden.contains(&element.id)
                && element
                    .rect
                    .is_some_and(|hidden| clip(rect, hidden).is_some())
        }) {
            return None;
        }
        Some(rect)
    }

    /// The parser's settings for the answer to this look: only listed ids, pixels inside
    /// the image that was sent.
    pub fn parser_settings(&self, language: &str, mode: Mode) -> Settings {
        Settings {
            language: language.to_owned(),
            mode,
            point_ids: self.list.ids.clone(),
            screenshot: Some(self.redacted.frame.sent_size()),
        }
    }

    /// Where a validated point is: the listed element's rectangle, or for a pixel the
    /// pointable element under it, clipped to the captured region. None when nothing is
    /// there (invariant 5: "I can't find it" beats pointing at a guess).
    pub fn resolve(&self, point: &Point) -> Option<PhysRect> {
        let rect = match point {
            Point::Id(id) => {
                if !self.list.ids.iter().any(|listed| listed == id) {
                    return None;
                }
                // Only something a person can act on: never the window itself, static
                // text or a disabled or hidden control (the same rule as `target_at`).
                let element = self.snapshot.element(id)?;
                let pointable = element.interactive
                    && element.parent.is_some()
                    && element.enabled
                    && !element.offscreen;
                if !pointable {
                    return None;
                }
                element.rect?
            }
            Point::Pixel(x, y) => {
                let shot = ShotPoint::new(f64::from(*x) + 0.5, f64::from(*y) + 0.5).ok()?;
                let phys = self.redacted.frame.shot_to_phys_checked(shot)?;
                if point_in_any(phys, &self.painted) {
                    return None;
                }
                self.snapshot.target_at(phys)?.rect?
            }
        };
        // Partly under a painted area: what the model saw there was black.
        if overlaps_any(rect, &self.painted) {
            return None;
        }
        clip(rect, self.snapshot.frame)
    }
}

/// Why there is no look; each maps to the line Mate says instead.
#[derive(Debug)]
pub enum LookFailure {
    /// No window to look at (the desktop, ours, minimized, another virtual desktop).
    NoWindow,
    /// UI Automation could not walk the window.
    Uia,
    /// The capture failed or came back black (protected content).
    Capture(mm_win32::capture::CaptureError),
    /// Redaction refused (fail closed).
    Prepare(LookError),
    /// The window moved, resized, opened or closed a popup, or lost the foreground while
    /// it was walked and copied: the masks would not match the picture.
    Changed,
    /// Our own windows could not be found, so they could not be hidden from the picture.
    OwnWindows,
    /// The question was cancelled (a newer press) before the copy.
    Cancelled,
}

impl std::fmt::Display for LookFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindow => f.write_str("no window to look at"),
            Self::Uia => f.write_str("UI Automation could not walk the window"),
            Self::Capture(error) => write!(f, "capture: {error}"),
            Self::Prepare(error) => write!(f, "{error}"),
            Self::Changed => f.write_str("the window changed during the look"),
            Self::OwnWindows => f.write_str("our windows are not known"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl LookFailure {
    /// The locale key Mate speaks instead of pointing.
    pub fn line(&self) -> &'static str {
        match self {
            Self::NoWindow => "robot.cant_find",
            Self::Capture(mm_win32::capture::CaptureError::Black) => "robot.protected_window",
            Self::Uia
            | Self::Capture(_)
            | Self::Prepare(_)
            | Self::Changed
            | Self::OwnWindows
            | Self::Cancelled => "robot.cant_look",
        }
    }
}

/// How long our hidden windows stay hidden before the copy: one composed frame.
const HIDDEN_FRAME: std::time::Duration = std::time::Duration::from_millis(50);

/// One look at a time: two overlapping looks would show and hide our windows in each
/// other's copy.
static LOOKING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Our windows hidden while the screen is copied, shown again however the copy ends.
struct Hidden<'a>(&'a [mm_win32::HWND]);

impl<'a> Hidden<'a> {
    fn new(windows: &'a [mm_win32::HWND]) -> Self {
        for &hwnd in windows {
            mm_win32::overlay::hide_for_capture(hwnd);
        }
        Self(windows)
    }
}

impl Drop for Hidden<'_> {
    fn drop(&mut self) {
        for &hwnd in self.0 {
            mm_win32::overlay::show_after_capture(hwnd);
        }
    }
}

/// One look at the foreground window (blocking: UIA and GDI; call it off the async
/// runtime). `own` are our windows' handles (Mate, the highlight; both required): never a
/// target, and hidden while the screen is copied. `wanted` is asked just before our
/// windows hide: a cancelled question never touches the screen. The window is checked
/// again after the copy; if it changed, the look is taken once more, then refused.
///
/// A tray question passes the last eligible window as `target` instead of following
/// tray focus; None looks at the foreground window.
pub fn take_look_at(
    own: &[isize],
    wanted: &dyn Fn() -> bool,
    target: Option<mm_platform::TargetWindow>,
) -> Result<Look, LookFailure> {
    use mm_win32::HWND;

    if own.len() < 2 || own.contains(&0) {
        return Err(LookFailure::OwnWindows);
    }
    let own: Vec<HWND> = own
        .iter()
        .map(|&h| HWND(h as *mut std::ffi::c_void))
        .collect();
    let _one = LOOKING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match look_once(&own, wanted, target) {
        Err(LookFailure::Changed) => look_once(&own, wanted, target),
        result => result,
    }
}

fn look_once(
    own: &[mm_win32::HWND],
    wanted: &dyn Fn() -> bool,
    remembered: Option<mm_platform::TargetWindow>,
) -> Result<Look, LookFailure> {
    use mm_platform::same_hidden;
    use mm_win32::{capture, overlay, uia, window};

    #[cfg(debug_assertions)]
    let started = std::time::Instant::now();
    let target_now = || match remembered {
        Some(target) => window::remembered_target(target, own),
        None => window::foreground_target(own),
    };
    let target = target_now().ok_or(LookFailure::NoWindow)?;
    let walker = uia::UiaWalker::new().map_err(|_| LookFailure::Uia)?;
    let mut snapshot = walker
        .snapshot_with_popups(
            target.hwnd,
            &target.popup_hwnds,
            target.region,
            uia::WalkLimits::default(),
        )
        .map_err(|_| LookFailure::Uia)?;
    // A cut walk (a browser page) lists what it saw; every field to hide comes from the
    // provider's own search instead (D50). Without it the look fails closed.
    let scanned = if snapshot.truncated {
        let found = scan(&walker, &target).ok_or(LookFailure::Uia)?;
        complete_with(&mut snapshot, &found);
        Some(found)
    } else {
        None
    };
    let occluders = window::occluders(&target, own);
    if !wanted() {
        return Err(LookFailure::Cancelled);
    }
    let copied = {
        let hidden = Hidden::new(own);
        // A hide that did not take would put Mate or the ring in the picture.
        if !hidden.0.iter().all(|&hwnd| overlay::is_hidden(hwnd)) {
            return Err(LookFailure::OwnWindows);
        }
        std::thread::sleep(HIDDEN_FRAME);
        // And the compositor has presented a frame without them.
        overlay::wait_for_composition();
        capture::capture_region(target.region)
    };
    let capture = copied.map_err(LookFailure::Capture)?;
    #[cfg(debug_assertions)]
    let copied_at = started.elapsed();
    // The masks come from the walk: the window must be exactly where it was then.
    let after = target_now().ok_or(LookFailure::Changed)?;
    if after.hwnd != target.hwnd
        || after.frame != target.frame
        || after.popups != target.popups
        || after.region != target.region
    {
        #[cfg(debug_assertions)]
        eprintln!(
            "look: changed: window {} frame {} popups {}/{} region {}",
            after.hwnd == target.hwnd,
            after.frame == target.frame,
            target.popups.len(),
            after.popups.len(),
            after.region == target.region,
        );
        return Err(LookFailure::Changed);
    }
    // Windows above the target before and after the copy are both painted: the taskbar
    // re-raises itself when our windows come back, and whatever was above during the
    // copy was above at one of the two moments.
    let mut occluders = occluders;
    for above in window::occluders(&after, own) {
        if !occluders.contains(&above) {
            occluders.push(above);
        }
    }
    // A password field that appeared or moved while the screen was copied is not in the
    // first masks (invariant 3): look again and compare what must be hidden.
    let unchanged = match &scanned {
        Some(first) => {
            let second = scan(&walker, &target).ok_or(LookFailure::Uia)?;
            signature(first) == signature(&second)
        }
        None => {
            let again = walker
                .snapshot_with_popups(
                    target.hwnd,
                    &target.popup_hwnds,
                    target.region,
                    uia::WalkLimits::default(),
                )
                .map_err(|_| LookFailure::Uia)?;
            // A second walk that no longer sees everything vouches for nothing.
            !again.truncated && same_hidden(&snapshot, &again)
        }
    };
    if !unchanged {
        #[cfg(debug_assertions)]
        eprintln!("look: changed: hidden fields differ between the walks");
        return Err(LookFailure::Changed);
    }
    let painted = painted_areas(target.region, target.frame, &target.popups, &occluders);
    require_visible_frame(target.frame, &painted)?;
    // Development builds: where a look's time goes (sizes and times only, never names).
    #[cfg(debug_assertions)]
    eprintln!(
        "look: {} elements, copy done at {} ms, second walk done at {} ms, {} painted areas",
        snapshot.elements.len(),
        copied_at.as_millis(),
        started.elapsed().as_millis(),
        painted.len()
    );
    let mut look = prepare(snapshot, &capture, &painted).map_err(LookFailure::Prepare)?;
    look.cut = scanned.is_some();
    look.window_frame = target.frame;
    Ok(look)
}

/// Every field to hide in the target window and its popups (`UiaWalker::sensitive`);
/// None when any part of the search failed.
fn scan(
    walker: &mm_win32::uia::UiaWalker,
    target: &mm_win32::window::Target,
) -> Option<Vec<mm_platform::UiElement>> {
    let mut found = walker.sensitive(target.hwnd)?;
    for &popup in &target.popup_hwnds {
        found.extend(walker.sensitive(popup)?);
    }
    Some(found)
}

/// The id prefix of fields the provider search added (`complete_with`): painted, never
/// listed in the prompt.
const SEARCHED: &str = "s";

/// The search's fields join the walk's elements (new ids, no parent: never pointable,
/// never named in the prompt), and the snapshot then vouches for the picture. A walked
/// element the search also found (the same control type at the same place) is marked too,
/// so its name and everything below it stay out of the prompt.
fn complete_with(snapshot: &mut UiSnapshot, found: &[mm_platform::UiElement]) {
    for walked in &mut snapshot.elements {
        let Some(rect) = walked.rect else {
            continue;
        };
        // The same element: the same type, mostly the same place (the two reads may differ
        // by a pixel or a scroll step). Hiding a neighbour too is safe.
        let same = |field: &&mm_platform::UiElement| {
            field.control_type == walked.control_type
                && field.rect.is_some_and(|other| mostly_overlap(rect, other))
        };
        if found.iter().any(|field| same(&field)) {
            walked.password_like = true;
        }
    }
    for (index, field) in found.iter().enumerate() {
        let mut field = field.clone();
        field.id = format!("{SEARCHED}{index}");
        field.parent = None;
        field.password_like = true;
        snapshot.elements.push(field);
    }
    snapshot.truncated = false;
}

/// Whether the overlap of `a` and `b` is more than half of the smaller one.
fn mostly_overlap(a: PhysRect, b: PhysRect) -> bool {
    let width = i64::from(a.right.min(b.right)) - i64::from(a.left.max(b.left));
    let height = i64::from(a.bottom.min(b.bottom)) - i64::from(a.top.max(b.top));
    if width <= 0 || height <= 0 {
        return false;
    }
    let area = |r: PhysRect| {
        (i64::from(r.right) - i64::from(r.left)) * (i64::from(r.bottom) - i64::from(r.top))
    };
    2 * width * height > area(a).min(area(b))
}

/// A field to hide as two searches compare it: its control type and its edges.
type FieldMark = (String, Option<(i32, i32, i32, i32)>);

/// What two searches are compared by: control types and bounds, in any order.
fn signature(found: &[mm_platform::UiElement]) -> Vec<FieldMark> {
    let mut signature: Vec<_> = found
        .iter()
        .map(|field| {
            (
                field.control_type.clone(),
                field.rect.map(|r| (r.left, r.top, r.right, r.bottom)),
            )
        })
        .collect();
    signature.sort();
    signature
}

/// `--look-probe`: walks the foreground window (after 3 s to switch to it) with several
/// limits and prints sizes and times, never names: how big real trees are.
pub fn probe() -> serde_json::Value {
    use mm_win32::{uia, window};
    std::thread::sleep(std::time::Duration::from_secs(3));
    let Some(target) = window::foreground_target(&[]) else {
        return serde_json::json!({"error": "no foreground window"});
    };
    let Ok(walker) = uia::UiaWalker::new() else {
        return serde_json::json!({"error": "UI Automation unavailable"});
    };
    let mut runs = Vec::new();
    for _ in 0..2 {
        let started = std::time::Instant::now();
        let sensitive = walker.sensitive(target.hwnd);
        runs.push(match &sensitive {
        Some(found) => serde_json::json!({
            "scan": "sensitive", "ms": started.elapsed().as_millis(), "found": found.len(),
            "password": found.iter().filter(|e| e.is_password).count(),
            "password_like": found.iter().filter(|e| e.password_like).count(),
            "offscreen": found.iter().filter(|e| e.offscreen).count(),
        }),
        None => serde_json::json!({"scan": "sensitive", "ms": started.elapsed().as_millis(), "error": true}),
    });
    }
    for (max_nodes, max_depth) in [(400, 12), (1500, 24), (5000, 60)] {
        let started = std::time::Instant::now();
        let walked = walker.snapshot_with_popups(
            target.hwnd,
            &target.popup_hwnds,
            target.region,
            uia::WalkLimits {
                max_nodes,
                max_depth,
            },
        );
        let ms = started.elapsed().as_millis();
        runs.push(match walked {
            Ok(snapshot) => {
                let parents: std::collections::HashMap<&str, Option<&str>> = snapshot
                    .elements
                    .iter()
                    .map(|e| (e.id.as_str(), e.parent.as_deref()))
                    .collect();
                let depth_of = |id: &str| {
                    let mut levels = 0;
                    let mut up = parents.get(id).copied().flatten();
                    while let Some(parent) = up {
                        levels += 1;
                        up = parents.get(parent).copied().flatten();
                    }
                    levels
                };
                let deepest = snapshot
                    .elements
                    .iter()
                    .map(|e| depth_of(&e.id))
                    .max()
                    .unwrap_or(0);
                serde_json::json!({
                    "max_nodes": max_nodes, "max_depth": max_depth, "ms": ms,
                    "elements": snapshot.elements.len(), "deepest": deepest,
                    "truncated": snapshot.truncated,
                    "interactive": snapshot.elements.iter().filter(|e| e.interactive).count(),
                })
            }
            Err(error) => serde_json::json!({"max_nodes": max_nodes, "error": error.to_string()}),
        });
    }
    serde_json::json!({"popups": target.popup_hwnds.len(), "region": format!("{:?}", target.region), "runs": runs})
}

fn require_visible_frame(frame: PhysRect, painted: &[PhysRect]) -> Result<(), LookFailure> {
    if mm_core::geom::uncovered(frame, painted).is_empty() {
        return Err(LookFailure::NoWindow);
    }
    Ok(())
}

fn clip(rect: PhysRect, to: PhysRect) -> Option<PhysRect> {
    PhysRect::new(
        rect.left.max(to.left),
        rect.top.max(to.top),
        rect.right.min(to.right),
        rect.bottom.min(to.bottom),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use mm_platform::{TargetWindow, UiElement};

    use super::*;

    const REGION: PhysRect = PhysRect {
        left: 100,
        top: 50,
        right: 500,
        bottom: 350,
    };

    #[test]
    fn completely_covered_frame_fails_but_a_visible_strip_passes() {
        let left = PhysRect::new(100, 50, 300, 350).unwrap();
        let right = PhysRect::new(300, 50, 500, 350).unwrap();
        assert!(matches!(
            require_visible_frame(REGION, &[left, right]),
            Err(LookFailure::NoWindow)
        ));
        let almost_right = PhysRect::new(300, 50, 499, 350).unwrap();
        assert!(require_visible_frame(REGION, &[left, almost_right]).is_ok());
        assert!(require_visible_frame(REGION, &[]).is_ok());
    }

    fn element(id: &str, control_type: &str, name: &str, rect: PhysRect) -> UiElement {
        UiElement {
            id: id.into(),
            parent: (id != "e0").then(|| "e0".into()),
            control_type: control_type.into(),
            name: name.into(),
            rect: Some(rect),
            interactive: mm_platform::is_interactive_type(control_type),
            enabled: true,
            offscreen: false,
            is_password: false,
            password_like: false,
        }
    }

    fn snapshot() -> UiSnapshot {
        let mut password = element(
            "e3",
            "Edit",
            "Password",
            PhysRect::new(150, 200, 350, 230).unwrap(),
        );
        password.is_password = true;
        UiSnapshot {
            target: TargetWindow {
                handle: 1,
                process_id: 2,
            },
            frame: REGION,
            elements: vec![
                element("e0", "Window", "Settings", REGION),
                element(
                    "e1",
                    "Button",
                    "Save",
                    PhysRect::new(400, 300, 480, 330).unwrap(),
                ),
                element(
                    "e2",
                    "Text",
                    "Display",
                    PhysRect::new(120, 70, 300, 90).unwrap(),
                ),
                password,
            ],
            truncated: false,
        }
    }

    fn capture() -> CapturedImage {
        CapturedImage::new(400, 300, vec![200; 400 * 300 * 4]).unwrap()
    }

    fn live_button() -> UiElement {
        let mut button = element(
            "p0",
            "Button",
            "Open",
            PhysRect::new(400, 300, 480, 330).unwrap(),
        );
        button.parent = None;
        button
    }

    #[test]
    fn live_hit_accepts_an_unlisted_button_without_ancestry() {
        let mut snapshot = snapshot();
        snapshot.elements.retain(|element| element.id != "e1");
        let look = prepare(snapshot, &capture(), &[]).unwrap();
        assert!(!look.cut);
        assert_eq!(look.resolve(&Point::Pixel(320, 260)), None);
        let button = live_button();
        assert_eq!(look.resolve_live(Some(button.clone())), button.rect);
        assert_eq!(look.resolve_live(None), None);
    }

    #[test]
    fn live_hit_refuses_noninteractive_disabled_and_offscreen_elements() {
        let look = prepare(snapshot(), &capture(), &[]).unwrap();
        let mut noninteractive = live_button();
        noninteractive.interactive = false;
        assert_eq!(look.resolve_live(Some(noninteractive)), None);
        let mut disabled = live_button();
        disabled.enabled = false;
        assert_eq!(look.resolve_live(Some(disabled)), None);
        let mut offscreen = live_button();
        offscreen.offscreen = true;
        assert_eq!(look.resolve_live(Some(offscreen)), None);
    }

    #[test]
    fn live_hit_refuses_each_password_flag() {
        let look = prepare(snapshot(), &capture(), &[]).unwrap();
        let mut password = live_button();
        password.is_password = true;
        assert_eq!(look.resolve_live(Some(password)), None);
        let mut password_like = live_button();
        password_like.password_like = true;
        assert_eq!(look.resolve_live(Some(password_like)), None);
    }

    #[test]
    fn live_hit_refuses_missing_invalid_or_outside_bounds() {
        let look = prepare(snapshot(), &capture(), &[]).unwrap();
        for rect in [
            None,
            Some(PhysRect {
                left: 400,
                top: 300,
                right: 400,
                bottom: 330,
            }),
            Some(PhysRect {
                left: 480,
                top: 300,
                right: 400,
                bottom: 330,
            }),
            Some(PhysRect::new(99, 300, 120, 330).unwrap()),
            Some(PhysRect::new(400, 49, 480, 60).unwrap()),
            Some(PhysRect::new(400, 300, 501, 330).unwrap()),
            Some(PhysRect::new(400, 300, 480, 351).unwrap()),
        ] {
            let mut button = live_button();
            button.rect = rect;
            assert_eq!(look.resolve_live(Some(button)), None, "{rect:?}");
        }
    }

    #[test]
    fn live_hit_refuses_even_partial_painted_overlap() {
        let look = prepare(
            snapshot(),
            &capture(),
            &[PhysRect::new(479, 329, 500, 350).unwrap()],
        )
        .unwrap();
        assert_eq!(look.resolve_live(Some(live_button())), None);
    }

    #[test]
    fn live_hit_refuses_hidden_fields_and_their_descendants() {
        let mut snapshot = snapshot();
        let mut child = element("e4", "Text", "", live_button().rect.unwrap());
        child.parent = Some("e3".into());
        snapshot.elements.push(child);
        let look = prepare(snapshot, &capture(), &[]).unwrap();
        assert_eq!(look.resolve_live(Some(live_button())), None);
        let mut button = live_button();
        // Only one corner overlaps the password field itself.
        button.rect = Some(PhysRect::new(349, 229, 400, 260).unwrap());
        assert_eq!(look.resolve_live(Some(button)), None);
    }

    #[test]
    fn live_hit_accepts_exactly_forty_percent_but_refuses_more() {
        let mut snapshot = snapshot();
        snapshot.elements.clear();
        let look = prepare(snapshot, &capture(), &[]).unwrap();
        let mut button = live_button();
        button.rect = Some(PhysRect::new(100, 50, 500, 170).unwrap());
        assert_eq!(look.resolve_live(Some(button.clone())), button.rect);
        button.rect = Some(PhysRect::new(100, 50, 500, 171).unwrap());
        assert_eq!(look.resolve_live(Some(button)), None);
    }

    #[test]
    fn the_password_is_painted_and_never_named() {
        let look = prepare(snapshot(), &capture(), &[]).unwrap();
        assert_eq!(look.redacted.painted, 1);
        assert!(look.list.text.contains("Save"));
        assert!(!look.list.text.contains("Password"));
        assert!(look.list.ids.contains(&"e1".to_owned()));
        assert_eq!(look.redacted.frame.sent_size(), (400, 300));
    }

    #[test]
    fn a_truncated_or_stale_snapshot_fails_closed() {
        let mut truncated = snapshot();
        truncated.truncated = true;
        assert!(matches!(
            prepare(truncated, &capture(), &[]),
            Err(LookError::Redaction(RedactionError::SnapshotTruncated))
        ));
        let small = CapturedImage::new(10, 10, vec![0; 400]).unwrap();
        assert!(matches!(
            prepare(snapshot(), &small, &[]),
            Err(LookError::Redaction(RedactionError::CaptureMismatch))
        ));
    }

    #[test]
    fn a_hidden_field_without_bounds_on_screen_is_refused() {
        let mut unlocated = snapshot();
        let mut field = element("e4", "Edit", "PIN", REGION);
        field.rect = None;
        field.password_like = true;
        unlocated.elements.push(field.clone());
        assert!(matches!(
            prepare(unlocated, &capture(), &[]),
            Err(LookError::UnlocatedSensitive)
        ));
        // Offscreen without bounds: not in the picture, nothing to paint.
        let mut offscreen = snapshot();
        field.offscreen = true;
        offscreen.elements.push(field);
        assert!(prepare(offscreen, &capture(), &[]).is_ok());
    }

    #[test]
    fn only_pointable_elements_resolve_by_id() {
        let look = prepare(snapshot(), &capture(), &[]).unwrap();
        // The window itself and static text are listed, but never targets.
        assert_eq!(look.resolve(&Point::Id("e0".into())), None);
        assert_eq!(look.resolve(&Point::Id("e2".into())), None);
    }

    #[test]
    fn points_resolve_to_listed_elements_or_to_nothing() {
        let look = prepare(snapshot(), &capture(), &[]).unwrap();
        assert_eq!(
            look.resolve(&Point::Id("e1".into())),
            Some(PhysRect::new(400, 300, 480, 330).unwrap())
        );
        assert_eq!(look.resolve(&Point::Id("e99".into())), None);
        // The Save button in screenshot pixels (same size as the region here).
        assert_eq!(
            look.resolve(&Point::Pixel(320, 260)),
            Some(PhysRect::new(400, 300, 480, 330).unwrap())
        );
        // Blank space is no target, and neither is a point off the image.
        assert_eq!(look.resolve(&Point::Pixel(10, 290)), None);
        assert_eq!(look.resolve(&Point::Pixel(4000, 10)), None);
        let settings = look.parser_settings("en", Mode::Calm);
        assert_eq!(settings.screenshot, Some((400, 300)));
        assert_eq!(settings.point_ids, look.list.ids);
    }

    #[test]
    fn nothing_under_another_window_is_named_or_pointed_at() {
        // Another app's window covers the Save button's corner.
        let other = PhysRect::new(380, 280, 500, 350).unwrap();
        let look = prepare(snapshot(), &capture(), &[other]).unwrap();
        assert_eq!(look.redacted.painted, 2);
        assert!(!look.list.text.contains("Save"));
        assert_eq!(look.resolve(&Point::Id("e1".into())), None);
        assert_eq!(look.resolve(&Point::Pixel(320, 260)), None);
        assert_eq!(look.resolve(&Point::Pixel(390, 290)), None);
    }

    #[test]
    fn a_searched_field_hides_its_walked_original_and_lifts_the_cut() {
        let mut walked = snapshot();
        walked.truncated = true;
        // The walk saw a revealed password as plain text, without anything marking it.
        let revealed = PhysRect::new(150, 250, 350, 280).unwrap();
        walked
            .elements
            .push(element("e4", "Text", "hunter2", revealed));
        let mut found = element("s0", "Text", "", revealed);
        found.password_like = true;
        complete_with(&mut walked, &[found]);
        assert!(!walked.truncated);
        let look = prepare(walked, &capture(), &[]).unwrap();
        assert!(!look.list.text.contains("hunter2"));
        assert_eq!(look.resolve(&Point::Id("s0".into())), None);
        assert!(!look.list.text.contains("s0 "));
    }
}
