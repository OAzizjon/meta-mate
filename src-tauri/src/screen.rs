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
    redact, same_areas,
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
    /// Window consent permits an unclassifiable snapshot.
    pub unclassified_allowed: bool,
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
#[cfg(test)]
pub fn prepare(
    snapshot: UiSnapshot,
    capture: &CapturedImage,
    painted: &[PhysRect],
) -> Result<Look, LookError> {
    let frame = snapshot.frame;
    prepare_at(snapshot, capture, painted, frame, false)
}

fn prepare_at(
    mut snapshot: UiSnapshot,
    capture: &CapturedImage,
    painted: &[PhysRect],
    window_frame: PhysRect,
    consent: bool,
) -> Result<Look, LookError> {
    for element in &mut snapshot.elements {
        if was_password(snapshot.target, window_frame, element) {
            element.password_like = true;
        }
    }
    let hidden = snapshot.hidden_ids();
    let must_paint = |element: &mm_platform::UiElement| {
        hidden.contains(&element.id)
            || must_hide(
                element.is_password,
                element.password_like,
                was_password(snapshot.target, window_frame, element),
            )
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
                was_password_this_session: was_password(snapshot.target, window_frame, element),
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
        // Consent only waives flat-window classification; ambiguous private edits
        // are in `hidden` and must already have passed the bounds check above.
        unclassified_sensitive_field: snapshot.flat() && !consent,
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
        unclassified_allowed: consent,
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

    /// What the X-ray look draws of this look (highlight.rs `scan`), as rectangles only:
    /// the listed elements' outlines, and every field painted over for privacy as a
    /// locked tile. Both clipped to the captured region; nothing under a painted area.
    pub fn scan(&self) -> (Vec<PhysRect>, Vec<PhysRect>) {
        // `inner` lies wholly within `outer`.
        fn inside(inner: PhysRect, outer: PhysRect) -> bool {
            inner.left >= outer.left
                && inner.top >= outer.top
                && inner.right <= outer.right
                && inner.bottom <= outer.bottom
        }
        let hidden = self.snapshot.hidden_ids();
        let shown = |rect: PhysRect| {
            if overlaps_any(rect, &self.painted) {
                return None;
            }
            clip(rect, self.snapshot.frame)
        };
        let area = |r: PhysRect| {
            (i64::from(r.right) - i64::from(r.left)) * (i64::from(r.bottom) - i64::from(r.top))
        };
        let mut fields: Vec<PhysRect> = self
            .snapshot
            .elements
            .iter()
            .filter(|element| !element.offscreen && hidden.contains(&element.id))
            // A field half under another window keeps its tile: only one hidden entirely is
            // dropped, so a lock never leaves part of a field bare.
            .filter_map(|element| {
                let rect = element.rect?;
                if self.painted.iter().any(|&over| inside(rect, over)) {
                    return None;
                }
                clip(rect, self.snapshot.frame)
            })
            .collect();
        // A field's own text and children sit inside it: one tile for them all.
        fields.sort_by_key(|&rect| std::cmp::Reverse(area(rect)));
        let mut locked: Vec<PhysRect> = Vec::new();
        for field in fields {
            if !locked.iter().any(|&kept| inside(field, kept)) {
                locked.push(field);
            }
        }
        let rects = self
            .list
            .ids
            .iter()
            .filter(|id| !hidden.contains(id.as_str()))
            .filter_map(|id| self.snapshot.element(id)?.rect)
            .filter_map(shown)
            .collect();
        (rects, locked)
    }

    /// The parser's settings for the answer to this look: only listed ids, pixels inside
    /// the image that was sent.
    pub fn parser_settings(&self, language: &str, mode: Mode) -> Settings {
        Settings {
            language: language.to_owned(),
            mode,
            point_ids: self.list.ids.clone(),
            screenshot: Some(self.redacted.frame.sent_size()),
            // Screen answers never open anything: a page could ask for it (prompt injection).
            open_request: None,
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
            Self::Prepare(LookError::Redaction(RedactionError::UnclassifiedSensitiveField)) => {
                "robot.private_window"
            }
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

static LOOK_CONSENT: std::sync::Mutex<mm_platform::LookConsent> =
    std::sync::Mutex::new(mm_platform::LookConsent::new());

/// Session-only geometry, never names or values. Without stable UIA RuntimeIds,
/// any same-type field in this window may be the password after a layout change.
/// A field seen as a password: its window, control type and rect relative to the frame.
type PasswordField = (mm_platform::TargetWindow, String, (i64, i64, i64, i64));

#[derive(Default)]
struct PasswordMemory(Vec<PasswordField>);

impl PasswordMemory {
    fn position(frame: PhysRect, element: &UiElement) -> Option<(i64, i64, i64, i64)> {
        element.rect.map(|r| {
            (
                i64::from(r.left) - i64::from(frame.left),
                i64::from(r.top) - i64::from(frame.top),
                i64::from(r.right) - i64::from(frame.left),
                i64::from(r.bottom) - i64::from(frame.top),
            )
        })
    }

    fn contains(
        &self,
        target: mm_platform::TargetWindow,
        frame: PhysRect,
        element: &UiElement,
    ) -> bool {
        let Some(_) = Self::position(frame, element) else {
            return false;
        };
        self.0
            .iter()
            .any(|(window, kind, _)| *window == target && *kind == element.control_type)
    }

    fn observe(
        &mut self,
        target: mm_platform::TargetWindow,
        frame: PhysRect,
        elements: &mut [UiElement],
    ) {
        for element in elements.iter().filter(|e| e.is_password) {
            let Some(position) = Self::position(frame, element) else {
                continue;
            };
            let key = (target, element.control_type.clone(), position);
            if !self.0.contains(&key) {
                self.0.push(key);
            }
        }
        for element in elements {
            if self.contains(target, frame, element) {
                element.password_like = true;
            }
        }
    }
}

static PASSWORDS: std::sync::Mutex<PasswordMemory> =
    std::sync::Mutex::new(PasswordMemory(Vec::new()));

fn remember_passwords(
    target: mm_platform::TargetWindow,
    frame: PhysRect,
    elements: &mut [UiElement],
) {
    PASSWORDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .observe(target, frame, elements);
}

fn was_password(target: mm_platform::TargetWindow, frame: PhysRect, element: &UiElement) -> bool {
    PASSWORDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(target, frame, element)
}

/// Restore only windows that were visible: Mate may be sleeping for an hour, or have been
/// hidden for one while the screen was copied.
struct Hidden(Vec<mm_win32::HWND>);

impl Hidden {
    fn new(windows: &[mm_win32::HWND]) -> Self {
        let visible: Vec<_> = windows
            .iter()
            .copied()
            .filter(|&hwnd| !mm_win32::overlay::is_hidden(hwnd))
            .collect();
        for &hwnd in &visible {
            mm_win32::overlay::hide_for_capture(hwnd);
        }
        Self(visible)
    }
}

impl Drop for Hidden {
    fn drop(&mut self) {
        for &hwnd in &self.0 {
            if !crate::presence::is_hidden(hwnd.0 as isize) {
                mm_win32::overlay::show_after_capture(hwnd);
            }
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
    confirm: &dyn Fn(mm_platform::TargetWindow) -> bool,
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
    match look_once(&own, wanted, target, confirm, false) {
        Err(LookFailure::Changed) => look_once(&own, wanted, target, confirm, false),
        result => result,
    }
}

/// Walks of a flat window before Mate asks, `FLAT_REWALK_EVERY` apart: Chromium's tree can
/// be poor on the first query (CLAUDE.md "Known pitfalls").
const FLAT_REWALKS: u32 = 2;
const FLAT_REWALK_EVERY: std::time::Duration = std::time::Duration::from_millis(300);

/// How long to wait before walking the window again after `rewalks` walks, or None to go on
/// with this one. Only a flat walk is retried, never a window that always asks
/// (`requires_consent`): no walk makes that one classifiable.
fn flat_rewalk(requires_consent: bool, flat: bool, rewalks: u32) -> Option<std::time::Duration> {
    (!requires_consent && flat && rewalks < FLAT_REWALKS).then_some(FLAT_REWALK_EVERY)
}

/// Clicks on Mate (drag.rs) while a consent question is shown: when each one's press
/// reached us. One question at a time (`LOOKING`).
static CONSENT_CLICKS: std::sync::Mutex<Option<std::sync::mpsc::Sender<std::time::Instant>>> =
    std::sync::Mutex::new(None);

/// The shown question's clicks; dropping it gives clicks on Mate back to talking.
struct ConsentClicks(std::sync::mpsc::Receiver<std::time::Instant>);

impl ConsentClicks {
    fn open() -> Self {
        let (send, receive) = std::sync::mpsc::channel();
        *consent_clicks() = Some(send);
        Self(receive)
    }
}

impl Drop for ConsentClicks {
    fn drop(&mut self) {
        *consent_clicks() = None;
    }
}

fn consent_clicks()
-> std::sync::MutexGuard<'static, Option<std::sync::mpsc::Sender<std::time::Instant>>> {
    CONSENT_CLICKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A click on Mate (drag.rs, on the UI thread) whose press reached us at `pressed`: true when
/// a consent question is shown and took it (`confirm_look`); false, and the click explains
/// or talks as before.
pub(crate) fn consent_clicked(pressed: std::time::Instant) -> bool {
    consent_clicks()
        .as_ref()
        .is_some_and(|clicks| clicks.send(pressed).is_ok())
}

/// Ask without taking focus. Only a fresh physical F8 in the target, or a fresh physical
/// click on Mate while the target is in front (his window never activates), grants consent.
pub(crate) fn confirm_look(
    app: &tauri::AppHandle,
    generation: u64,
    target: mm_platform::TargetWindow,
    wanted: &dyn Fn() -> bool,
) -> bool {
    use std::time::{Duration, Instant};
    use tauri::Manager;

    use crate::answer_stream::{AnswerMsg, AnswerStream};
    let (send, receive) = std::sync::mpsc::channel();
    let Ok(_hook) = mm_win32::keyboard::start(send) else {
        return false;
    };
    // Without the mouse hook no click can be shown to be a person's, and the question asks
    // for a click: refuse (fail closed) instead of showing a question nobody can answer.
    let (mouse_send, presses) = std::sync::mpsc::channel();
    let Ok(_mouse) = mm_win32::mouse::start(mouse_send) else {
        return false;
    };
    let clicks = ConsentClicks::open();
    let stream = app.state::<AnswerStream>();
    stream.send(AnswerMsg::Line {
        generation,
        key: "robot.look_consent",
    });
    let offered = Instant::now();
    let mut released = false;
    // Recent left presses (when, injected), oldest first: a double click on Mate sends two
    // clicks, and each must find its own press (Gemini #1461).
    let mut recent: std::collections::VecDeque<(Instant, bool)> = std::collections::VecDeque::new();
    while wanted() && offered.elapsed() < Duration::from_secs(30) {
        let front = mm_win32::window::foreground_target(&[]).map(|front| front.window);
        if front != Some(target) {
            return false;
        }
        // A press's hook event is queued before its click can reach us: take the click first.
        let clicked = clicks.0.try_recv().ok();
        for event in presses.try_iter() {
            if event.button == mm_win32::mouse::Button::Left && event.down {
                if recent.len() == 8 {
                    recent.pop_front();
                }
                recent.push_back((event.time, event.injected));
            }
        }
        if let Some(clicked) = clicked {
            // This click's press: the newest one before it reached us.
            let press = recent.iter().rev().find(|(at, _)| *at < clicked).copied();
            if let Some(answer) = consent_click_answer(target, offered, press, clicked, front) {
                return answer;
            }
            // Presses up to this click are used up by it, answered or not.
            recent.retain(|(at, _)| *at >= clicked);
        }
        if let Ok(event) = receive.recv_timeout(Duration::from_millis(50))
            && let Some(answer) = consent_key_answer(target, offered, event, &mut released)
        {
            return answer;
        }
    }
    false
}

fn consent_key_answer(
    target: mm_platform::TargetWindow,
    offered: std::time::Instant,
    event: mm_win32::keyboard::KeyEvent,
    released: &mut bool,
) -> Option<bool> {
    if event.injected || event.at <= offered {
        return None;
    }
    let Some(front) = event.foreground else {
        return Some(false);
    };
    if front.handle != target.handle || front.process_id != target.process_id {
        return Some(false);
    }
    if event.vk == 0x1b && event.down {
        return Some(false);
    }
    if event.vk == 0x77 {
        if !event.down {
            *released = true;
        } else if *released {
            return Some(true);
        }
    }
    None
}

/// A click meant for something else (talking to Mate just as the question appeared) must
/// not say yes: a press counts only this long after the question was sent (Meta #1447).
const CONSENT_READ_MS: u64 = 700;
/// The press must be this click's own: a physical press elsewhere long before a click that
/// reached Mate without passing the mouse hook is no answer (Meta #1447).
const CONSENT_PRESS_MAX_MS: u64 = 1500;

/// A click on Mate that reached us at `clicked` answers as F8 does: yes only when `press`,
/// the last left press the mouse hook saw (when, injected), was a person's, at least
/// `CONSENT_READ_MS` after the question and at most `CONSENT_PRESS_MAX_MS` before the click;
/// no when the target is not in front. Another click (software's, one held since before the
/// question, one too soon to have been read) is no answer.
fn consent_click_answer(
    target: mm_platform::TargetWindow,
    offered: std::time::Instant,
    press: Option<(std::time::Instant, bool)>,
    clicked: std::time::Instant,
    front: Option<mm_platform::TargetWindow>,
) -> Option<bool> {
    if front != Some(target) {
        return Some(false);
    }
    press
        .is_some_and(|(at, injected)| {
            !injected
                && at >= offered + std::time::Duration::from_millis(CONSENT_READ_MS)
                && at < clicked
                && clicked.duration_since(at)
                    <= std::time::Duration::from_millis(CONSENT_PRESS_MAX_MS)
        })
        .then_some(true)
}

fn look_once(
    own: &[mm_win32::HWND],
    wanted: &dyn Fn() -> bool,
    remembered: Option<mm_platform::TargetWindow>,
    confirm: &dyn Fn(mm_platform::TargetWindow) -> bool,
    consent: bool,
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
    let consent = consent
        || LOOK_CONSENT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .granted(target.window, window::exists);
    let walker = uia::UiaWalker::new().map_err(|_| LookFailure::Uia)?;
    let walk = || {
        walker
            .snapshot_with_popups(
                target.hwnd,
                &target.popup_hwnds,
                target.region,
                uia::WalkLimits::default(),
            )
            .map_err(|_| LookFailure::Uia)
    };
    let mut snapshot = walk()?;
    remember_passwords(snapshot.target, target.frame, &mut snapshot.elements);
    // A flat first walk may only be Chromium still building its tree: walk the same window
    // again before asking (nothing is copied in between).
    let mut rewalks = 0;
    while !consent
        && let Some(delay) = flat_rewalk(target.requires_consent, snapshot.flat(), rewalks)
    {
        std::thread::sleep(delay);
        if !wanted() {
            return Err(LookFailure::Cancelled);
        }
        if target_now().as_ref() != Some(&target) {
            return Err(LookFailure::Changed);
        }
        snapshot = walk()?;
        remember_passwords(snapshot.target, target.frame, &mut snapshot.elements);
        rewalks += 1;
    }
    // Check the actual walk before scan results can make a flat tree look complete.
    // Ambiguous private edits need no consent: they are always painted (`hidden_ids`).
    let needs_consent = target.requires_consent || snapshot.flat();
    if !consent && needs_consent {
        if !wanted() || !confirm(target.window) || !wanted() {
            return Err(LookFailure::Cancelled);
        }
        if target_now().as_ref() != Some(&target) {
            return Err(LookFailure::Changed);
        }
        // Rewalk after the answer: permission never makes an old mask current.
        LOOK_CONSENT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .answer(target.window, true);
        return look_once(own, wanted, Some(target.window), confirm, true);
    }
    let cut = snapshot.truncated;
    let scanned = scan(&walker, &target).ok_or(LookFailure::Uia)?;
    complete_with(&mut snapshot, &scanned);
    let occluders = window::occluders(&target, own);
    if !wanted() {
        return Err(LookFailure::Cancelled);
    }
    if consent && !window::foreground_target(own).is_some_and(|front| front.window == target.window)
    {
        return Err(LookFailure::Changed);
    }
    let copied = {
        let hidden = Hidden::new(own);
        // A hide that did not take would put Mate or the ring in the picture.
        if !hidden.0.iter().all(|&hwnd| overlay::is_hidden(hwnd)) {
            return Err(LookFailure::OwnWindows);
        }
        std::thread::sleep(HIDDEN_FRAME);
        // Windows above the target just before the copied frame is composed and just after
        // the copy: one that came or went in between may be in the picture yet in neither
        // the read before nor the one after it, so any change fails closed (invariant 3).
        let around = window::occluders(&target, own);
        // The compositor has presented a frame without our windows.
        overlay::wait_for_composition();
        let copied = capture::capture_region(target.region);
        if !same_areas(&around, &window::occluders(&target, own)) {
            return Err(LookFailure::Changed);
        }
        copied.map(|capture| (capture, around))
    };
    let (capture, around) = copied.map_err(LookFailure::Capture)?;
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
    // Windows above the target before, around and after the copy are all painted: the
    // taskbar re-raises itself when our windows come back.
    let mut occluders = occluders;
    for above in around.into_iter().chain(window::occluders(&after, own)) {
        if !occluders.contains(&above) {
            occluders.push(above);
        }
    }
    // A password field that appeared or moved while the screen was copied is not in the
    // first masks (invariant 3): look again and compare what must be hidden.
    let mut again = walker
        .snapshot_with_popups(
            target.hwnd,
            &target.popup_hwnds,
            target.region,
            uia::WalkLimits::default(),
        )
        .map_err(|_| LookFailure::Uia)?;
    remember_passwords(again.target, target.frame, &mut again.elements);
    let second = scan(&walker, &target).ok_or(LookFailure::Uia)?;
    let unclassifiable = again.flat();
    complete_with(&mut again, &second);
    // Both reads feed memory before comparing, including a newly revealed password.
    remember_passwords(snapshot.target, target.frame, &mut snapshot.elements);
    let unchanged = (!unclassifiable || consent)
        && signature(&scanned) == signature(&second)
        && same_hidden(&snapshot, &again);
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
    let mut look = prepare_at(snapshot, &capture, &painted, target.frame, consent)
        .map_err(LookFailure::Prepare)?;
    look.cut = cut;
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
    remember_passwords(target.window, target.frame, &mut found);
    Some(found)
}

/// The id prefix of fields the provider search added (`complete_with`): painted, never
/// listed in the prompt.
const SEARCHED: &str = "s";

/// The search's fields join the walk's elements (new ids, no parent: never pointable,
/// never named in the prompt), and the snapshot then vouches for the picture. The
/// provider search must classify contextual inputs before returning this complete list.
/// A walked element the search also found (the same control type overlapping it) is marked
/// too, so its name and everything below it stay out of the prompt.
fn complete_with(snapshot: &mut UiSnapshot, found: &[mm_platform::UiElement]) {
    for walked in &mut snapshot.elements {
        let Some(rect) = walked.rect else {
            continue;
        };
        // The same element: the same type, overlapping at all (the two reads may differ by
        // a pixel or a scroll step, and the part outside the found rect is not painted
        // otherwise). Hiding a neighbour too is safe.
        let same = |field: &&mm_platform::UiElement| {
            field.control_type == walked.control_type
                && field.rect.is_some_and(|other| overlaps_any(rect, &[other]))
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
    fn opaque_telegram_and_raster_login_require_one_look_consent() {
        for title in ["Telegram", "Telegram - Login code", "Bank - Microsoft Edge"] {
            let mut page = snapshot();
            page.elements = vec![element("root", "Window", title, REGION)];
            complete_with(&mut page, &[]);
            assert!(
                page.flat(),
                "an empty successful scan does not prove coverage"
            );
            assert!(prepare(page.clone(), &capture(), &[]).is_err());
            assert!(prepare_at(page, &capture(), &[], REGION, true).is_ok());
        }
        let mut page = snapshot();
        page.elements = vec![
            element("root", "Window", "Login", REGION),
            element("document", "Document", "", REGION),
            element(
                "heading",
                "Text",
                "Welcome",
                PhysRect::new(110, 60, 200, 80).unwrap(),
            ),
            element(
                "raster",
                "Image",
                "",
                PhysRect::new(100, 90, 500, 350).unwrap(),
            ),
        ];
        page.elements[1].parent = Some("root".into());
        page.elements[2].parent = Some("document".into());
        page.elements[3].parent = Some("document".into());
        assert!(prepare(page, &capture(), &[]).is_err());
    }

    #[test]
    fn contextual_payment_label_keeps_postal_code_pixels() {
        let mut page = snapshot();
        page.elements = vec![
            element("e0", "Window", "Checkout", REGION),
            element("e1", "Document", "", REGION),
            element(
                "e2",
                "Text",
                "Payment",
                PhysRect::new(150, 170, 250, 190).unwrap(),
            ),
            element(
                "e3",
                "Edit",
                "Postal code",
                PhysRect::new(150, 200, 350, 230).unwrap(),
            ),
        ];
        for element in &mut page.elements[2..] {
            element.parent = Some("e1".into());
        }
        let look = prepare(page, &capture(), &[]).unwrap();
        let expected = mm_core::shot::png_data_url(
            &mm_core::shot::encode_png_rgb(&vec![200; 400 * 300 * 3], 400, 300).unwrap(),
        );
        let body = mm_core::api::sam::sam_point_request(
            0,
            0,
            look.redacted.frame.sent_size(),
            &look.redacted.image,
        );
        assert_eq!(
            body["input"][0]["content"][1]["image_url"].as_str(),
            Some(expected.as_str())
        );
        assert!(look.list.text.contains("Postal code"));
    }

    #[test]
    fn look_consent_requires_new_physical_keys_in_the_same_window() {
        use mm_core::teacher::KeyWindow;
        use mm_win32::keyboard::KeyEvent;
        use std::time::{Duration, Instant};
        let target = TargetWindow {
            handle: 8,
            process_id: 9,
        };
        let offered = Instant::now();
        let mut event = KeyEvent {
            foreground: Some(KeyWindow {
                handle: 8,
                process_id: 9,
            }),
            vk: 0x77,
            down: true,
            injected: false,
            at: offered + Duration::from_millis(1),
        };
        let mut released = false;
        assert_eq!(
            consent_key_answer(target, offered, event, &mut released),
            None
        );
        event.down = false;
        event.injected = true;
        assert_eq!(
            consent_key_answer(target, offered, event, &mut released),
            None
        );
        assert!(!released);
        event.injected = false;
        event.at = offered;
        assert_eq!(
            consent_key_answer(target, offered, event, &mut released),
            None
        );
        assert!(!released);
        event.at += Duration::from_millis(1);
        assert_eq!(
            consent_key_answer(target, offered, event, &mut released),
            None
        );
        assert!(released);
        event.down = true;
        assert_eq!(
            consent_key_answer(target, offered, event, &mut released),
            Some(true)
        );
        event.foreground.as_mut().unwrap().process_id += 1;
        assert_eq!(
            consent_key_answer(target, offered, event, &mut released),
            Some(false)
        );
        event.foreground.as_mut().unwrap().process_id -= 1;
        event.vk = 0x1b;
        assert_eq!(
            consent_key_answer(target, offered, event, &mut released),
            Some(false)
        );
        event.foreground = None;
        assert_eq!(
            consent_key_answer(target, offered, event, &mut released),
            Some(false)
        );
    }

    #[test]
    fn a_click_on_mate_says_yes_only_for_a_new_physical_press_in_the_same_window() {
        use std::time::{Duration, Instant};
        let target = TargetWindow {
            handle: 8,
            process_id: 9,
        };
        let offered = Instant::now();
        let pressed = offered + Duration::from_millis(CONSENT_READ_MS + 5);
        let clicked = pressed + Duration::from_millis(15);
        let front = Some(target);
        // Too soon after the question to have been read: a click meant for talking.
        assert_eq!(
            consent_click_answer(
                target,
                offered,
                Some((
                    offered + Duration::from_millis(CONSENT_READ_MS - 100),
                    false
                )),
                clicked,
                front
            ),
            None
        );
        // A press long before this click is another click's.
        let late = pressed + Duration::from_millis(CONSENT_PRESS_MAX_MS + 1);
        assert_eq!(
            consent_click_answer(target, offered, Some((pressed, false)), late, front),
            None
        );
        assert_eq!(
            consent_click_answer(target, offered, Some((pressed, false)), clicked, front),
            Some(true)
        );
        // No press seen (no mouse hook) or software's: no answer, the question stays.
        assert_eq!(
            consent_click_answer(target, offered, None, clicked, front),
            None
        );
        assert_eq!(
            consent_click_answer(target, offered, Some((pressed, true)), clicked, front),
            None
        );
        // Pressed when the question came, though released and delivered after it.
        assert_eq!(
            consent_click_answer(target, offered, Some((offered, false)), clicked, front),
            None
        );
        // A press after this click reached us belongs to another click.
        assert_eq!(
            consent_click_answer(target, offered, Some((clicked, false)), clicked, front),
            None
        );
        let other = TargetWindow {
            process_id: 10,
            ..target
        };
        for front in [Some(other), None] {
            assert_eq!(
                consent_click_answer(target, offered, Some((pressed, false)), clicked, front),
                Some(false)
            );
        }
    }

    #[test]
    fn clicks_on_mate_go_to_the_consent_question_only_while_it_is_shown() {
        let pressed = std::time::Instant::now();
        assert!(!consent_clicked(pressed));
        let clicks = ConsentClicks::open();
        assert!(consent_clicked(pressed));
        assert_eq!(clicks.0.try_recv().ok(), Some(pressed));
        drop(clicks);
        assert!(!consent_clicked(pressed));
    }

    #[test]
    fn only_a_flat_walk_is_walked_again_twice_before_asking() {
        let every = std::time::Duration::from_millis(300);
        assert_eq!(flat_rewalk(false, true, 0), Some(every));
        assert_eq!(flat_rewalk(false, true, 1), Some(every));
        assert_eq!(flat_rewalk(false, true, 2), None);
        assert_eq!(flat_rewalk(false, false, 0), None);
        // A window that always asks gains nothing from another walk.
        assert_eq!(flat_rewalk(true, true, 0), None);
        assert_eq!(flat_rewalk(true, false, 0), None);
    }

    #[test]
    fn permission_keeps_password_masks_and_fail_closed_redaction_checks() {
        let mut page = snapshot();
        page.elements
            .retain(|element| element.id == "e0" || element.id == "e3");
        assert!(page.unclassifiable());
        let look = prepare_at(page.clone(), &capture(), &[], REGION, true).unwrap();
        assert_eq!(look.redacted.painted, 1);
        assert!(!look.list.text.contains("Password"));
        assert!(prepare(page.clone(), &capture(), &[]).is_err());
        page.elements
            .iter_mut()
            .find(|e| e.id == "e3")
            .unwrap()
            .rect = None;
        assert!(matches!(
            prepare_at(page, &capture(), &[], REGION, true),
            Err(LookError::UnlocatedSensitive)
        ));
        let mut page = snapshot();
        page.truncated = true;
        assert!(prepare_at(page, &capture(), &[], REGION, true).is_err());
    }

    #[test]
    fn revealed_password_with_empty_or_innocuous_name_is_always_painted() {
        let field = PhysRect::new(150, 200, 350, 230).unwrap();
        let mut page = snapshot();
        page.elements = vec![
            element("e0", "Window", "Microsoft Edge", REGION),
            element("e1", "Document", "Page", REGION),
            element(
                "e2",
                "Text",
                "Password",
                PhysRect::new(150, 170, 250, 190).unwrap(),
            ),
            element("e3", "Edit", "", field),
        ];
        page.elements[2].parent = Some("e1".into());
        page.elements[3].parent = Some("e1".into());
        assert!(!page.flat());
        assert!(page.ambiguous_private_edits().contains("e3"));
        assert!(!page.elements[3].is_password);
        assert!(!page.elements[3].password_like);

        // The sent PNG must match pixels painted black over the edit and its 4 px
        // padding, with every other pixel still the capture's original grey.
        let mut expected = vec![200; 400 * 300 * 3];
        for y in 146..184 {
            for x in 46..254 {
                let offset = (y * 400 + x) * 3;
                expected[offset..offset + 3].fill(0);
            }
        }
        let expected_url = mm_core::shot::png_data_url(
            &mm_core::shot::encode_png_rgb(&expected, 400, 300).unwrap(),
        );
        for (name, consent) in ["", "Enter here", "Login code", "Email code"]
            .into_iter()
            .flat_map(|name| [false, true].map(|consent| (name, consent)))
        {
            page.elements[3].name = name.into();
            if matches!(name, "Login code" | "Email code") {
                // Authentication-code names alone must override login/email purposes.
                page.elements[2].name = "Heading".into();
            }
            let look = prepare_at(page.clone(), &capture(), &[], REGION, consent).unwrap();
            assert_eq!(look.redacted.painted, 1);
            assert!(look.list.text.contains("e3 Edit [hidden]"));
            // Mate may still point at where to type, as at any password field; its
            // pixels and contents never leave.
            assert_eq!(look.resolve(&Point::Id("e3".into())), Some(field));
            let body = mm_core::api::sam::sam_point_request(
                0,
                0,
                look.redacted.frame.sent_size(),
                &look.redacted.image,
            );
            assert_eq!(
                body["input"][0]["content"][1]["image_url"].as_str(),
                Some(expected_url.as_str()),
                "consent={consent}"
            );
            let mut unlocated = page.clone();
            unlocated.elements[3].rect = None;
            assert!(matches!(
                prepare_at(unlocated, &capture(), &[], REGION, consent),
                Err(LookError::UnlocatedSensitive)
            ));
        }
    }

    #[test]
    fn approved_sparse_windows_can_use_sam_away_from_masks() {
        let mut page = snapshot();
        page.elements.retain(|element| element.id == "e0");
        assert!(page.unclassifiable());
        let painted = PhysRect::new(100, 50, 150, 100).unwrap();
        let mut look = prepare_at(page, &capture(), &[painted], REGION, true).unwrap();
        assert!(crate::sam::outline_allowed(&look, 200, 150));
        assert!(!crate::sam::outline_allowed(&look, 10, 10));
        look.unclassified_allowed = false;
        assert!(!crate::sam::outline_allowed(&look, 200, 150));
    }

    #[test]
    fn password_memory_survives_reveal_and_window_movement_but_is_scoped() {
        let mut memory = PasswordMemory::default();
        let mut snapshot = snapshot();
        memory.observe(snapshot.target, snapshot.frame, &mut snapshot.elements);
        let field = snapshot
            .elements
            .iter_mut()
            .find(|e| e.is_password)
            .unwrap();
        field.is_password = false;
        field.password_like = false;
        memory.observe(snapshot.target, snapshot.frame, &mut snapshot.elements);
        let field = snapshot
            .elements
            .iter()
            .find(|e| e.control_type == "Edit")
            .unwrap();
        assert!(field.password_like);
        let mut shifted = field.clone();
        shifted.rect = shifted.rect.map(|r| PhysRect {
            left: r.left + 50,
            right: r.right + 50,
            ..r
        });
        let frame = PhysRect {
            left: snapshot.frame.left + 50,
            right: snapshot.frame.right + 50,
            ..snapshot.frame
        };
        assert!(memory.contains(snapshot.target, frame, &shifted));
        // Reflow and resizing move the field within an unchanged window too.
        shifted.rect = Some(PhysRect::new(200, 100, 450, 140).unwrap());
        assert!(memory.contains(snapshot.target, snapshot.frame, &shifted));
        let mut other = snapshot.target;
        other.handle += 1;
        assert!(!memory.contains(other, frame, &shifted));
        other = snapshot.target;
        other.process_id += 1;
        assert!(!memory.contains(other, frame, &shifted));
        shifted.rect = None;
        assert!(!memory.contains(snapshot.target, frame, &shifted));
        shifted.is_password = true;
        let mut empty = PasswordMemory::default();
        empty.observe(snapshot.target, frame, &mut [shifted]);
        assert!(empty.0.is_empty());
    }

    #[test]
    fn text_over_a_hidden_field_never_reaches_the_prompt() {
        let mut snapshot = snapshot();
        let field = snapshot
            .elements
            .iter()
            .find(|e| e.is_password)
            .unwrap()
            .rect
            .unwrap();
        snapshot
            .elements
            .push(element("peek", "Text", "revealed-secret", field));
        let look = prepare(snapshot, &capture(), &[]).unwrap();
        assert!(!look.list.text.contains("revealed-secret"));
    }

    #[test]
    fn the_scan_outlines_what_was_listed_and_locks_what_was_painted() {
        let mut page = snapshot();
        let field = PhysRect::new(150, 200, 350, 230).unwrap();
        let save = PhysRect::new(400, 300, 480, 330).unwrap();
        // Text over the password and an offscreen field: no tiles of their own.
        page.elements
            .push(element("peek", "Text", "revealed-secret", field));
        let mut offscreen = element(
            "e9",
            "Edit",
            "PIN",
            PhysRect::new(110, 60, 140, 80).unwrap(),
        );
        offscreen.password_like = true;
        offscreen.offscreen = true;
        page.elements.push(offscreen);
        let look = prepare(page.clone(), &capture(), &[]).unwrap();
        let (rects, locked) = look.scan();
        assert_eq!(locked, vec![field]);
        // The window, Save and the text; the listed password is a tile, not an outline.
        assert_eq!(look.list.ids.len(), 4);
        assert_eq!(rects.len(), 3);
        assert!(rects.contains(&save) && rects.contains(&REGION));
        assert!(!rects.contains(&field));
        // Under another app's window: neither outlined nor locked.
        let other = PhysRect::new(380, 280, 500, 350).unwrap();
        let (rects, locked) = prepare(page, &capture(), &[other]).unwrap().scan();
        assert_eq!(rects, vec![PhysRect::new(120, 70, 300, 90).unwrap()]);
        assert_eq!(locked, vec![field]);
        let look = prepare(snapshot(), &capture(), &[field]).unwrap();
        assert!(look.scan().1.is_empty());
        // Half under another window: the tile stays, so no part of the field is left bare.
        let half = PhysRect::new(300, 190, 400, 240).unwrap();
        let look = prepare(snapshot(), &capture(), &[half]).unwrap();
        assert_eq!(look.scan().1, vec![field]);
    }

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
        let mut snapshot = UiSnapshot {
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
        };
        // Supply eight content nodes; window chrome never satisfies flatness.
        for index in 0..5 {
            snapshot.elements.push(element(
                &format!("content{index}"),
                "Text",
                "Detail",
                PhysRect::new(120 + index * 10, 100, 125 + index * 10, 110).unwrap(),
            ));
        }
        snapshot
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
        // No password field to overlap; retain the classifiable content tree.
        snapshot.elements[3].is_password = false;
        snapshot.elements[3].control_type = "Text".into();
        snapshot.elements[3].name = "Page".into();
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

    #[test]
    fn a_searched_field_overlapping_its_walked_original_only_in_part_still_hides_it() {
        let mut walked = snapshot();
        // The two reads disagree: the found field covers only 40 % of the walked one.
        let revealed = PhysRect::new(150, 250, 350, 280).unwrap();
        walked
            .elements
            .push(element("e4", "Text", "hunter2", revealed));
        let shifted = PhysRect::new(270, 250, 470, 280).unwrap();
        let mut found = element("s0", "Text", "", shifted);
        found.password_like = true;
        complete_with(&mut walked, &[found]);
        assert!(walked.element("e4").is_some_and(|e4| e4.password_like));
        let look = prepare(walked, &capture(), &[]).unwrap();
        assert!(!look.list.text.contains("hunter2"));
    }

    #[test]
    fn contextual_provider_field_beyond_a_long_article_is_painted_after_completion() {
        let field = PhysRect::new(150, 200, 350, 230).unwrap();
        let mut walked = snapshot();
        walked.elements = vec![
            element("root", "Window", "Microsoft Edge", REGION),
            element("document", "Document", "Article", REGION),
        ];
        for i in 0..501 {
            let mut text = element(
                &format!("text{i}"),
                "Text",
                "Article paragraph",
                PhysRect::new(110, 100, 490, 120).unwrap(),
            );
            text.parent = Some("document".into());
            walked.elements.push(text);
        }
        walked.truncated = true;
        // Neither the revealed input nor its separate label reached the bounded walk.
        // The complete provider scan classifies this input using the label.
        let mut found = element("input", "Edit", "Enter here", field);
        found.password_like = true;
        let mut again = walked.clone();
        complete_with(&mut walked, &[found.clone()]);
        complete_with(&mut again, &[found]);
        assert!(!walked.truncated);
        assert!(mm_platform::same_hidden(&walked, &again));
        let look = prepare(walked, &capture(), &[]).unwrap();
        assert!(!look.list.text.contains("Enter here"));
        let mut expected = vec![200; 400 * 300 * 3];
        for y in 146..184 {
            for x in 46..254 {
                let offset = (y * 400 + x) * 3;
                expected[offset..offset + 3].fill(0);
            }
        }
        let expected_url = mm_core::shot::png_data_url(
            &mm_core::shot::encode_png_rgb(&expected, 400, 300).unwrap(),
        );
        let body = mm_core::api::sam::sam_point_request(
            0,
            0,
            look.redacted.frame.sent_size(),
            &look.redacted.image,
        );
        assert_eq!(
            body["input"][0]["content"][1]["image_url"].as_str(),
            Some(expected_url.as_str()),
        );
    }
}
