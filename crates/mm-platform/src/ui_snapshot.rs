//! A flat snapshot of the target's UI Automation tree, as in `fixtures/uia/*.json`.

use std::fmt;

use mm_core::geom::{PhysPoint, PhysRect};

use crate::target::TargetWindow;

/// Control types a user acts on (the same list as `tools/mm_fixture.py`).
const INTERACTIVE_TYPES: &[&str] = &[
    "Button",
    "Hyperlink",
    "MenuItem",
    "ListItem",
    "TabItem",
    "CheckBox",
    "RadioButton",
    "ComboBox",
    "Edit",
    "TreeItem",
    "SplitButton",
    "Slider",
    "Spinner",
    "DataItem",
];

/// Whether a UIA control type (without the `ControlType.` prefix) is one a user acts on.
pub fn is_interactive_type(control_type: &str) -> bool {
    INTERACTIVE_TYPES.contains(&control_type)
}

/// The target window and its owned popups at one moment, elements in tree order.
#[derive(Debug, Clone)]
pub struct UiSnapshot {
    /// The window this snapshot was taken from.
    pub target: TargetWindow,
    /// The region it covers in physical pixels: the window plus its owned popups (the
    /// fixture's `shot_frame`, which is also the screenshot's extent).
    pub frame: PhysRect,
    /// Every walked element; parents come before their children.
    pub elements: Vec<UiElement>,
    /// The walk hit its node or depth limit, or completeness is unknown. Redaction cannot be
    /// guaranteed then (invariant 3: fail closed).
    pub truncated: bool,
}

impl UiSnapshot {
    /// The element with this snapshot-local id, e.g. "e11" from `<point e11>`.
    pub fn element(&self, id: &str) -> Option<&UiElement> {
        self.elements.iter().find(|element| element.id == id)
    }

    /// The element under `point`, like `hit_test` in `tools/mm_fixture.py`: among enabled,
    /// on-screen elements whose rect contains the point, interactive ones win, then the
    /// smallest, then the first in tree order. Outside `frame` nothing is known: `None`.
    pub fn element_at(&self, point: PhysPoint) -> Option<&UiElement> {
        if !self.frame.contains(point) {
            return None;
        }
        self.elements
            .iter()
            .filter(|element| element.enabled && !element.offscreen)
            .filter_map(|element| {
                let rect = element.rect.as_ref()?;
                rect.contains(point).then_some((element, area(rect)))
            })
            .min_by_key(|(element, size)| (!element.interactive, *size))
            .map(|(element, _)| element)
    }

    /// What Mate may point at: like `element_at`, but only an interactive element below
    /// a root (never the window or a popup root itself). Blank space is `None`, so a
    /// point on nothing becomes "I can't find it" instead of the whole window
    /// (invariant 5).
    pub fn target_at(&self, point: PhysPoint) -> Option<&UiElement> {
        self.element_at(point)
            .filter(|element| element.interactive && element.parent.is_some())
    }
}

fn area(rect: &PhysRect) -> i64 {
    let width = i64::from(rect.right) - i64::from(rect.left);
    let height = i64::from(rect.bottom) - i64::from(rect.top);
    width.saturating_mul(height)
}

/// One UI Automation element.
#[derive(Clone)]
pub struct UiElement {
    /// Snapshot-local id such as "e11".
    pub id: String,
    /// The parent's id; `None` for the window and for each popup root.
    pub parent: Option<String>,
    /// UIA control type without the `ControlType.` prefix, e.g. "RadioButton".
    pub control_type: String,
    /// Accessible name. It can hold private text, so `Debug` prints only its length.
    pub name: String,
    /// Bounds in physical virtual-screen pixels; `None` when UIA reports them empty.
    pub rect: Option<PhysRect>,
    /// `is_interactive_type(control_type)`.
    pub interactive: bool,
    pub enabled: bool,
    pub offscreen: bool,
    /// UIA `IsPassword`; unknown counts as true (fail closed).
    pub is_password: bool,
    /// The recorder's password-word heuristic, which also catches revealed passwords (D37);
    /// unknown counts as true.
    pub password_like: bool,
}

/// Metadata only: names can be chat lines or document text (convention 3).
impl fmt::Debug for UiElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiElement")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("control_type", &self.control_type)
            .field("name_chars", &self.name.chars().count())
            .field("rect", &self.rect)
            .field("interactive", &self.interactive)
            .field("enabled", &self.enabled)
            .field("offscreen", &self.offscreen)
            .field("is_password", &self.is_password)
            .field("password_like", &self.password_like)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PhysRect {
        PhysRect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn element(id: &str, control_type: &str, bounds: PhysRect) -> UiElement {
        UiElement {
            id: id.to_owned(),
            parent: None,
            control_type: control_type.to_owned(),
            name: "Secret chat line".to_owned(),
            rect: Some(bounds),
            interactive: is_interactive_type(control_type),
            enabled: true,
            offscreen: false,
            is_password: false,
            password_like: false,
        }
    }

    fn hit(snapshot: &UiSnapshot, x: i32, y: i32) -> Option<&str> {
        snapshot
            .element_at(PhysPoint { x, y })
            .map(|element| element.id.as_str())
    }

    #[test]
    fn interactive_then_smallest_wins_and_disabled_is_skipped() {
        let mut disabled = element("e4", "Button", rect(12, 12, 14, 14));
        disabled.enabled = false;
        let snapshot = UiSnapshot {
            target: TargetWindow {
                handle: 1,
                process_id: 1,
            },
            frame: rect(-100, 0, 100, 100),
            elements: vec![
                element("e0", "Window", rect(-100, 0, 100, 100)),
                element("e1", "Button", rect(10, 10, 50, 30)),
                element("e2", "Text", rect(12, 12, 20, 20)),
                element("e3", "Button", rect(30, 15, 40, 25)),
                disabled,
            ],
            truncated: false,
        };
        assert_eq!(hit(&snapshot, 13, 13), Some("e1"));
        assert_eq!(hit(&snapshot, 35, 20), Some("e3"));
        assert_eq!(hit(&snapshot, -50, 50), Some("e0"));
        assert_eq!(hit(&snapshot, 150, 50), None);
        assert!(snapshot.element("e2").is_some() && snapshot.element("e9").is_none());
    }

    #[test]
    fn debug_hides_the_name() {
        let text = format!("{:?}", element("e2", "Text", rect(0, 0, 1, 1)));
        assert!(text.contains("e2") && !text.contains("Secret"));
    }

    #[test]
    fn targets_are_interactive_children_never_the_window_or_blank_space() {
        let child = |id: &str, kind: &str, bounds: PhysRect| {
            let mut e = element(id, kind, bounds);
            e.parent = Some("e0".to_owned());
            e
        };
        let snapshot = UiSnapshot {
            target: TargetWindow {
                handle: 1,
                process_id: 1,
            },
            frame: rect(0, 0, 100, 100),
            elements: vec![
                element("e0", "Window", rect(0, 0, 100, 100)),
                child("e1", "Button", rect(10, 10, 50, 30)),
                child("e2", "Text", rect(60, 60, 80, 80)),
            ],
            truncated: false,
        };
        let target = |x, y| {
            snapshot
                .target_at(PhysPoint { x, y })
                .map(|e| e.id.as_str())
        };
        assert_eq!(target(20, 20), Some("e1"));
        assert_eq!(target(70, 70), None, "plain text is not a target");
        assert_eq!(target(90, 5), None, "blank space is not the window");
    }
}
