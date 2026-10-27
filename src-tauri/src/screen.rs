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
use mm_core::redaction::{Field, Redacted, RedactionError, RedactionInput, redact};
use mm_core::shot::Bgra;
use mm_platform::{CapturedImage, UiSnapshot};

/// The long edge of the image the model gets (S4 measured ~3.7k tokens at 1280 px).
pub const MAX_EDGE: u32 = 1280;
/// Elements listed in the prompt: interactive ones first (render_element_list).
pub const MAX_LISTED: usize = 60;

/// One look at the target, ready for a point request.
pub struct Look {
    pub snapshot: UiSnapshot,
    pub redacted: Redacted,
    pub list: ElementList,
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
}

impl std::fmt::Display for LookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Redaction(error) => write!(f, "redaction refused: {error}"),
            Self::Capture => f.write_str("the capture is not a BGRA image"),
        }
    }
}

impl std::error::Error for LookError {}

/// Redacts `capture` (taken of `snapshot.frame`) and lists the elements the model may
/// point at. Every element UIA reports as a password, or that looks like one (D37), is
/// painted over and its name never reaches the prompt.
pub fn prepare(snapshot: UiSnapshot, capture: &CapturedImage) -> Result<Look, LookError> {
    let fields: Vec<Field> = snapshot
        .elements
        .iter()
        .filter_map(|element| {
            Some(Field {
                rect: element.rect?,
                is_password: element.is_password,
                password_like: element.password_like,
                was_password_this_session: false,
            })
        })
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
        .filter(|element| !element.offscreen)
        .filter_map(|element| {
            ElementLine::new(
                &element.id,
                &element.control_type,
                &element.name,
                element.rect?,
                redacted.frame,
                element.interactive,
                must_hide(element.is_password, element.password_like, false),
            )
        })
        .collect();
    let list = render_element_list(&lines, MAX_LISTED);
    Ok(Look {
        snapshot,
        redacted,
        list,
    })
}

impl Look {
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
                self.snapshot.element(id)?.rect?
            }
            Point::Pixel(x, y) => {
                let shot = ShotPoint::new(f64::from(*x) + 0.5, f64::from(*y) + 0.5).ok()?;
                let phys = self.redacted.frame.shot_to_phys_checked(shot)?;
                self.snapshot.target_at(phys)?.rect?
            }
        };
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
}

impl std::fmt::Display for LookFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindow => f.write_str("no window to look at"),
            Self::Uia => f.write_str("UI Automation could not walk the window"),
            Self::Capture(error) => write!(f, "capture: {error}"),
            Self::Prepare(error) => write!(f, "{error}"),
        }
    }
}

impl LookFailure {
    /// The locale key Mate speaks instead of pointing.
    pub fn line(&self) -> &'static str {
        match self {
            Self::NoWindow => "robot.cant_find",
            Self::Capture(mm_win32::capture::CaptureError::Black) => "robot.protected_window",
            Self::Uia | Self::Capture(_) | Self::Prepare(_) => "robot.cant_look",
        }
    }
}

/// How long our hidden windows stay hidden before the copy: one composed frame.
const HIDDEN_FRAME: std::time::Duration = std::time::Duration::from_millis(50);

/// One look at the foreground window (blocking: UIA and GDI; call it off the async
/// runtime). `own` are our windows' handles (Mate, the highlight): never a target, and
/// hidden while the screen is copied so they are not in the picture.
pub fn take_look(own: &[isize]) -> Result<Look, LookFailure> {
    use mm_win32::HWND;
    use mm_win32::{capture, overlay, uia, window};

    let own: Vec<HWND> = own
        .iter()
        .map(|&h| HWND(h as *mut std::ffi::c_void))
        .collect();
    let target = window::foreground_target(&own).ok_or(LookFailure::NoWindow)?;
    let walker = uia::UiaWalker::new().map_err(|_| LookFailure::Uia)?;
    let snapshot = walker
        .snapshot_with_popups(
            target.hwnd,
            &target.popup_hwnds,
            target.region,
            uia::WalkLimits::default(),
        )
        .map_err(|_| LookFailure::Uia)?;
    for &hwnd in &own {
        overlay::hide_for_capture(hwnd);
    }
    std::thread::sleep(HIDDEN_FRAME);
    let copied = capture::capture_region(target.region);
    for &hwnd in &own {
        overlay::show_after_capture(hwnd);
    }
    let capture = copied.map_err(LookFailure::Capture)?;
    prepare(snapshot, &capture).map_err(LookFailure::Prepare)
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

    #[test]
    fn the_password_is_painted_and_never_named() {
        let look = prepare(snapshot(), &capture()).unwrap();
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
            prepare(truncated, &capture()),
            Err(LookError::Redaction(RedactionError::SnapshotTruncated))
        ));
        let small = CapturedImage::new(10, 10, vec![0; 400]).unwrap();
        assert!(matches!(
            prepare(snapshot(), &small),
            Err(LookError::Redaction(RedactionError::CaptureMismatch))
        ));
    }

    #[test]
    fn points_resolve_to_listed_elements_or_to_nothing() {
        let look = prepare(snapshot(), &capture()).unwrap();
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
}
