//! Rust → the highlight windows: the ring around the element Mate points at, the beam from
//! Mate and the step badge (src/highlight.ts is the other end). Each monitor has its own
//! window covering its area, so everything sent is CSS pixels relative to that monitor
//! (invariant 2).

use std::fmt;

use mm_core::geom::{CssPoint, CssRect, GeomError, MonitorTransform, PhysPoint, PhysRect};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// The highlight windows' labels start with this, then the monitor's index (overlay.rs);
/// the event they listen to.
const WINDOW_PREFIX: &str = "highlight-";
const EVENT: &str = "highlight";
/// The highest step total the window draws a badge for (src/highlight.ts drops the
/// whole message otherwise).
const MAX_STEPS: u32 = 20;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum HighlightMsg {
    /// The ring around `rect`, the beam from `beam_from` and the "2 / 3" badge for `step`.
    Target {
        rect: Rect,
        beam_from: Option<Point>,
        step: Option<Step>,
    },
    /// Everything fades out.
    Clear,
}

/// CSS pixels relative to the highlight window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// CSS pixels relative to the highlight window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Step {
    pub number: u32,
    pub total: u32,
}

impl From<CssRect> for Rect {
    fn from(rect: CssRect) -> Self {
        Self {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }
    }
}

impl From<CssPoint> for Point {
    fn from(point: CssPoint) -> Self {
        Self {
            x: point.x,
            y: point.y,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HighlightError {
    /// The target's center is not on the given monitor (the caller picks the one it is
    /// on): nothing is drawn.
    OtherMonitor,
    /// An empty rectangle or an invalid monitor scale.
    Geometry(GeomError),
    /// Not 1 <= number <= total <= 20: the window would drop the whole message.
    StepOutOfRange,
    /// The event did not reach the window.
    Emit(String),
}

impl fmt::Display for HighlightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OtherMonitor => f.write_str("the target is not on the highlight's monitor"),
            Self::Geometry(error) => write!(f, "highlight geometry: {error}"),
            Self::StepOutOfRange => f.write_str("highlight step out of range"),
            Self::Emit(error) => write!(f, "highlight event: {error}"),
        }
    }
}

impl std::error::Error for HighlightError {}

impl From<GeomError> for HighlightError {
    fn from(error: GeomError) -> Self {
        Self::Geometry(error)
    }
}

/// The label of the highlight window covering monitor `index` of
/// `mm_win32::monitors::monitors()`.
pub fn label(index: usize) -> String {
    format!("{WINDOW_PREFIX}{index}")
}

/// Draws the ring around `target` on monitor `monitor_index` (`monitor`, the one its
/// highlight window covers), the beam from `mate` and the badge for `step` (number, total)
/// when given. One ring at a time: the other monitors' windows fade out.
#[cfg(windows)]
pub fn show(
    app: &AppHandle,
    monitor_index: usize,
    monitor: &mm_win32::monitors::Monitor,
    target: PhysRect,
    mate: Option<PhysRect>,
    step: Option<(u32, u32)>,
) -> Result<(), HighlightError> {
    let payload = message(monitor.area, monitor.scale(), target, mate, step)?;
    let shown = label(monitor_index);
    let others = send_all(app, |other| other != shown, &HighlightMsg::Clear);
    send(app, &shown, payload).and(others)
}

/// Fades the ring, the beam and the badge out on every monitor.
pub fn clear(app: &AppHandle) -> Result<(), HighlightError> {
    send_all(app, |_| true, &HighlightMsg::Clear)
}

fn send(app: &AppHandle, label: &str, payload: HighlightMsg) -> Result<(), HighlightError> {
    app.emit_to(label, EVENT, payload)
        .map_err(|error| HighlightError::Emit(error.to_string()))
}

/// `payload` to every highlight window whose label is `wanted`; all are tried, the first
/// failure is returned.
fn send_all(
    app: &AppHandle,
    wanted: impl Fn(&str) -> bool,
    payload: &HighlightMsg,
) -> Result<(), HighlightError> {
    let mut result = Ok(());
    for label in app.webview_windows().into_keys() {
        if label.starts_with(WINDOW_PREFIX) && wanted(label.as_str()) {
            result = result.and(send(app, &label, payload.clone()));
        }
    }
    result
}

/// The message for `target` on the monitor covering `monitor_area` at `scale`. A target
/// whose center is on another monitor is refused; one that only pokes past the edge is
/// clipped to the monitor, and the beam aims at what is left.
fn message(
    monitor_area: PhysRect,
    scale: f64,
    target: PhysRect,
    mate: Option<PhysRect>,
    step: Option<(u32, u32)>,
) -> Result<HighlightMsg, HighlightError> {
    let area = valid(monitor_area)?;
    let target = valid(target)?;
    if !area.contains(target.center()) {
        return Err(HighlightError::OtherMonitor);
    }
    let visible = PhysRect::new(
        target.left.max(area.left),
        target.top.max(area.top),
        target.right.min(area.right),
        target.bottom.min(area.bottom),
    )
    .map_err(|_| HighlightError::OtherMonitor)?;
    let origin = PhysPoint {
        x: area.left,
        y: area.top,
    };
    let transform = MonitorTransform::new(origin, scale)?;
    let beam_from = match mate {
        Some(mate) => {
            let edge = facing_edge(valid(mate)?, visible);
            Some(Point::from(transform.phys_to_css(edge)))
        }
        None => None,
    };
    let step = match step {
        Some((number, total)) if (1..=total).contains(&number) && total <= MAX_STEPS => {
            Some(Step { number, total })
        }
        Some(_) => return Err(HighlightError::StepOutOfRange),
        None => None,
    };
    Ok(HighlightMsg::Target {
        rect: transform.phys_rect_to_css(visible).into(),
        beam_from,
        step,
    })
}

/// Revalidates, because public rectangle fields allow struct literals.
fn valid(rect: PhysRect) -> Result<PhysRect, GeomError> {
    PhysRect::new(rect.left, rect.top, rect.right, rect.bottom)
}

/// The middle of Mate's edge that faces `target`: the axis with the wider gap between the
/// two rectangles decides, left/right before above/below on a tie.
fn facing_edge(mate: PhysRect, target: PhysRect) -> PhysPoint {
    let gap = |a: i32, b: i32| i64::from(a) - i64::from(b);
    // How far the target lies past each of Mate's edges (negative where they overlap).
    let right = gap(target.left, mate.right);
    let left = gap(mate.left, target.right);
    let below = gap(target.top, mate.bottom);
    let above = gap(mate.top, target.bottom);
    let middle = mate.center();
    if right.max(left) >= below.max(above) {
        let x = if right >= left { mate.right } else { mate.left };
        PhysPoint { x, y: middle.y }
    } else {
        let y = if below >= above {
            mate.bottom
        } else {
            mate.top
        };
        PhysPoint { x: middle.x, y }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PhysRect {
        PhysRect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn css(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn point(x: f64, y: f64) -> Option<Point> {
        Some(Point { x, y })
    }

    /// A 1920x1080 primary monitor at 100 %.
    fn primary() -> PhysRect {
        rect(0, 0, 1920, 1080)
    }

    fn button() -> PhysRect {
        rect(400, 400, 500, 450)
    }

    /// `target` on the primary monitor, without Mate or a step.
    fn at(target: PhysRect) -> Result<HighlightMsg, HighlightError> {
        message(primary(), 1.0, target, None, None)
    }

    /// Where the beam starts for Mate at `mate` and the button.
    fn beam(mate: PhysRect) -> Result<Option<Point>, HighlightError> {
        match message(primary(), 1.0, button(), Some(mate), None)? {
            HighlightMsg::Target { beam_from, .. } => Ok(beam_from),
            HighlightMsg::Clear => Ok(None),
        }
    }

    fn step(number: u32, total: u32) -> Result<Option<Step>, HighlightError> {
        match message(primary(), 1.0, button(), None, Some((number, total)))? {
            HighlightMsg::Target { step, .. } => Ok(step),
            HighlightMsg::Clear => Ok(None),
        }
    }

    #[test]
    fn maps_to_css_of_a_scaled_monitor_left_of_the_primary() {
        // 150 % at -1920,0: CSS = (physical - origin) / 1.5.
        let area = rect(-1920, 0, 0, 1080);
        let target = rect(-1500, 300, -1350, 450);
        let mate = rect(-1800, 270, -1590, 480);
        assert_eq!(
            message(area, 1.5, target, Some(mate), Some((2, 3))),
            Ok(HighlightMsg::Target {
                rect: css(280.0, 200.0, 380.0, 300.0),
                beam_from: point(220.0, 250.0),
                step: Some(Step {
                    number: 2,
                    total: 3,
                }),
            })
        );
    }

    #[test]
    fn without_mate_or_step_sends_no_beam_and_no_badge() {
        assert_eq!(
            at(button()),
            Ok(HighlightMsg::Target {
                rect: css(400.0, 400.0, 500.0, 450.0),
                beam_from: None,
                step: None,
            })
        );
    }

    #[test]
    fn beam_starts_at_the_middle_of_the_edge_facing_the_target() {
        // The button is 400,400-500,450: Mate right of, left of, below and above it.
        assert_eq!(beam(rect(600, 300, 800, 500)), Ok(point(600.0, 400.0)));
        assert_eq!(beam(rect(100, 300, 300, 500)), Ok(point(300.0, 400.0)));
        assert_eq!(beam(rect(350, 600, 550, 800)), Ok(point(450.0, 600.0)));
        assert_eq!(beam(rect(350, 100, 550, 300)), Ok(point(450.0, 300.0)));
        // Up and to the left: the wider gap (horizontal, 200 against 20) decides.
        assert_eq!(beam(rect(0, 0, 200, 380)), Ok(point(200.0, 190.0)));
    }

    #[test]
    fn refuses_a_target_centered_on_another_monitor() {
        let refused = Err(HighlightError::OtherMonitor);
        assert_eq!(at(rect(2000, 100, 2100, 150)), refused);
        assert_eq!(at(rect(-50, -50, -10, -10)), refused);
        // Mostly past the right edge: its center is on the next monitor.
        assert_eq!(at(rect(1900, 100, 2000, 150)), refused);
    }

    #[test]
    fn clips_a_target_that_pokes_past_the_monitor_edge() {
        assert_eq!(
            at(rect(1850, 100, 1950, 150)),
            Ok(HighlightMsg::Target {
                rect: css(1850.0, 100.0, 1920.0, 150.0),
                beam_from: None,
                step: None,
            })
        );
    }

    #[test]
    fn refuses_empty_rectangles() {
        let empty = Err(HighlightError::Geometry(GeomError::EmptyRect));
        assert_eq!(at(rect(400, 400, 400, 450)), empty);
        assert_eq!(
            message(rect(0, 0, 0, 1080), 1.0, button(), None, None),
            empty
        );
        assert_eq!(
            beam(rect(600, 300, 590, 500)),
            Err(HighlightError::Geometry(GeomError::EmptyRect))
        );
    }

    #[test]
    fn refuses_a_bad_scale() {
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                message(primary(), scale, button(), None, None),
                Err(HighlightError::Geometry(GeomError::InvalidScale))
            );
        }
    }

    #[test]
    fn sends_only_steps_the_window_draws() {
        assert_eq!(
            step(2, 3),
            Ok(Some(Step {
                number: 2,
                total: 3,
            }))
        );
        assert_eq!(step(1, 1).map(|step| step.is_some()), Ok(true));
        assert_eq!(step(20, 20).map(|step| step.is_some()), Ok(true));
        for (number, total) in [(0, 3), (4, 3), (0, 0), (21, 21)] {
            assert_eq!(step(number, total), Err(HighlightError::StepOutOfRange));
        }
    }

    #[test]
    fn labels_each_window_by_its_monitors_index() {
        assert_eq!(label(0), "highlight-0");
        assert_eq!(label(12), "highlight-12");
    }

    #[test]
    fn serializes_the_shape_the_window_expects() {
        let shown = HighlightMsg::Target {
            rect: css(1.0, 2.0, 3.5, 4.0),
            beam_from: point(5.0, 6.0),
            step: Some(Step {
                number: 2,
                total: 3,
            }),
        };
        assert_eq!(
            serde_json::to_value(&shown).ok(),
            Some(json!({
                "type": "target",
                "rect": {"left": 1.0, "top": 2.0, "right": 3.5, "bottom": 4.0},
                "beamFrom": {"x": 5.0, "y": 6.0},
                "step": {"number": 2, "total": 3}
            }))
        );
        let bare = HighlightMsg::Target {
            rect: css(1.0, 2.0, 3.5, 4.0),
            beam_from: None,
            step: None,
        };
        assert_eq!(
            serde_json::to_value(&bare).ok(),
            Some(json!({
                "type": "target",
                "rect": {"left": 1.0, "top": 2.0, "right": 3.5, "bottom": 4.0},
                "beamFrom": null,
                "step": null
            }))
        );
        assert_eq!(
            serde_json::to_value(&HighlightMsg::Clear).ok(),
            Some(json!({"type": "clear"}))
        );
    }
}
