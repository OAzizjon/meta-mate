//! A flat snapshot of the target's UI Automation tree, as in `fixtures/uia/*.json`.

use std::collections::{BTreeMap, BTreeSet};
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

/// Control types a password's show/hide toggle comes as (a button, a check box, a link,
/// an icon). A label or the field itself named "Password" is not a toggle: counting them
/// would hide every text beside any password field.
const TOGGLE_TYPES: [&str; 5] = ["Button", "CheckBox", "Hyperlink", "Custom", "Image"];

impl UiSnapshot {
    /// Fewer than eight nodes can be just window chrome; no field-bearing controls
    /// means UIA has not supplied enough evidence to classify the content safely.
    pub fn unclassifiable(&self) -> bool {
        let flat = self.elements.len() < 8
            && !self
                .elements
                .iter()
                .any(|e| matches!(e.control_type.as_str(), "Edit" | "Document" | "Custom"));
        let private_page = self.elements.iter().any(|e| {
            let name = e.name.to_lowercase();
            [
                "login",
                "log in",
                "sign in",
                "sign-in",
                "payment",
                "card",
                "вход",
                "войти",
                "оплат",
                "платеж",
                "платёж",
                "карт",
            ]
            .iter()
            .any(|word| name.contains(word))
        });
        flat || (private_page
            && self
                .elements
                .iter()
                .any(|e| e.control_type == "Edit" && e.name.trim().is_empty()))
    }

    /// Ids whose pixels and names must be hidden, including revealed password siblings
    /// and all descendants. Only links to existing parents establish a relationship.
    pub fn hidden_ids(&self) -> BTreeSet<String> {
        let ids: BTreeSet<&str> = self
            .elements
            .iter()
            .map(|element| element.id.as_str())
            .collect();
        let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut toggle_parents = BTreeSet::new();
        for element in &self.elements {
            if let Some(parent) = element
                .parent
                .as_deref()
                .filter(|parent| ids.contains(parent))
            {
                children
                    .entry(parent)
                    .or_default()
                    .push(element.id.as_str());
                if TOGGLE_TYPES.contains(&element.control_type.as_str())
                    && mm_core::privacy::password_toggle(&element.name)
                {
                    toggle_parents.insert(parent);
                }
            }
        }

        let mut hidden = BTreeSet::new();
        let mut pending = Vec::new();
        for element in &self.elements {
            let toggle_sibling = matches!(
                element.control_type.as_str(),
                "Edit" | "Document" | "Custom" | "Text"
            ) && element
                .parent
                .as_deref()
                .is_some_and(|parent| toggle_parents.contains(parent));
            if (mm_core::api::request::must_hide(element.is_password, element.password_like, false)
                || toggle_sibling)
                && hidden.insert(element.id.clone())
            {
                pending.push(element.id.as_str());
            }
        }
        // Each id is queued once, even if malformed parent links form a cycle.
        while let Some(parent) = pending.pop() {
            if let Some(descendants) = children.get(parent) {
                for &child in descendants {
                    if hidden.insert(child.to_owned()) {
                        pending.push(child);
                    }
                }
            }
        }
        let fields: Vec<_> = self
            .elements
            .iter()
            .filter(|e| hidden.contains(&e.id))
            .filter_map(|e| e.rect)
            .collect();
        for element in &self.elements {
            if element
                .rect
                .is_some_and(|rect| fields.iter().any(|&field| mostly_inside(rect, field)))
            {
                hidden.insert(element.id.clone());
            }
        }
        hidden
    }

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

/// More than half of the element's own area is covered by a hidden field.
pub fn mostly_inside(rect: PhysRect, field: PhysRect) -> bool {
    let width =
        (i128::from(rect.right.min(field.right)) - i128::from(rect.left.max(field.left))).max(0);
    let height =
        (i128::from(rect.bottom.min(field.bottom)) - i128::from(rect.top.max(field.top))).max(0);
    let area = (i128::from(rect.right) - i128::from(rect.left))
        * (i128::from(rect.bottom) - i128::from(rect.top));
    area > 0 && 2 * width * height > area
}

/// Whether both walks found the same hidden control types at identical bounds.
/// `mm-win32/src/uia.rs` assigns `e{elements.len()}` during each walk, so ids are
/// snapshot-local, not stable RuntimeIds. Compare (control_type, rect) instead,
/// independent of walk order, retaining duplicates so new hidden fields count.
/// Names are deliberately excluded: they may contain revealed password text.
pub fn same_hidden(before: &UiSnapshot, after: &UiSnapshot) -> bool {
    type Edges = (i32, i32, i32, i32);
    fn signature(snapshot: &UiSnapshot) -> Vec<(&str, Option<Edges>)> {
        let hidden = snapshot.hidden_ids();
        let mut elements: Vec<_> = snapshot
            .elements
            .iter()
            .filter(|element| hidden.contains(&element.id))
            .map(|element| {
                (
                    element.control_type.as_str(),
                    element.rect.map(|r| (r.left, r.top, r.right, r.bottom)),
                )
            })
            .collect();
        elements.sort_unstable();
        elements
    }
    signature(before) == signature(after)
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

    #[test]
    fn hidden_overlap_uses_the_elements_area_and_strict_half() {
        let field = rect(0, 0, 100, 100);
        assert!(mostly_inside(rect(10, 10, 20, 20), field));
        assert!(!mostly_inside(rect(0, 0, 200, 100), field));
        assert!(!mostly_inside(rect(-100, -100, 200, 200), field));
        assert!(!mostly_inside(rect(100, 0, 200, 100), field));
        assert!(!mostly_inside(rect(0, 0, 0, 0), field));
    }

    #[test]
    fn sparse_tree_and_unlabelled_private_edit_require_consent() {
        let mut snapshot = UiSnapshot {
            target: crate::TargetWindow {
                handle: 99,
                process_id: 99,
            },
            frame: rect(0, 0, 100, 100),
            elements: Vec::new(),
            truncated: false,
        };
        assert!(snapshot.unclassifiable());
        let element = UiElement {
            id: "root".into(),
            parent: None,
            control_type: "Window".into(),
            name: "Settings".into(),
            rect: Some(snapshot.frame),
            interactive: false,
            enabled: true,
            offscreen: false,
            is_password: false,
            password_like: false,
        };
        snapshot.elements = vec![element.clone(); 7];
        assert!(snapshot.unclassifiable());
        snapshot.elements.push(element);
        assert!(!snapshot.unclassifiable());
        snapshot.elements.truncate(1);
        snapshot.elements[0].control_type = "Edit".into();
        snapshot.elements[0].name.clear();
        assert!(!snapshot.unclassifiable());
        for title in [
            "Login",
            "Sign in",
            "Payment",
            "Card",
            "Вход",
            "Оплата",
            "Карта",
        ] {
            let mut root = snapshot.elements[0].clone();
            root.control_type = "Window".into();
            root.name = title.into();
            snapshot.elements.push(root);
            assert!(snapshot.unclassifiable(), "{title}");
            snapshot.elements.pop();
        }
        for kind in ["Document", "Custom"] {
            snapshot.elements[0].control_type = kind.into();
            assert!(!snapshot.unclassifiable());
        }
    }

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

    fn child(id: &str, parent: &str, control_type: &str) -> UiElement {
        let mut child = element(id, control_type, rect(0, 0, 10, 10));
        child.parent = Some(parent.to_owned());
        child
    }

    fn hidden(mut elements: Vec<UiElement>) -> BTreeSet<String> {
        // These tests isolate parent/toggle rules. Give each element distinct bounds;
        // geometric hiding is exercised separately below.
        for (index, element) in elements.iter_mut().enumerate() {
            let left = i32::try_from(index).unwrap() * 10;
            element.rect = Some(rect(left, 0, left + 5, 5));
        }
        UiSnapshot {
            target: TargetWindow {
                handle: 1,
                process_id: 1,
            },
            frame: rect(0, 0, 100, 100),
            elements,
            truncated: false,
        }
        .hidden_ids()
    }

    fn ids(expected: &[&str]) -> BTreeSet<String> {
        expected.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn hidden_ids_covers_unrelated_overlaid_names_but_not_large_containers() {
        let mut password = element("password", "Edit", rect(10, 10, 50, 30));
        password.is_password = true;
        let snapshot = snapshot(vec![
            password,
            element("peek", "Text", rect(15, 12, 40, 28)),
            element("half", "Text", rect(30, 10, 70, 30)),
            element("container", "Pane", rect(0, 0, 100, 100)),
        ]);
        assert_eq!(snapshot.hidden_ids(), ids(&["password", "peek"]));
    }

    fn snapshot(elements: Vec<UiElement>) -> UiSnapshot {
        UiSnapshot {
            target: TargetWindow {
                handle: 1,
                process_id: 1,
            },
            frame: rect(0, 0, 100, 100),
            elements,
            truncated: false,
        }
    }

    #[test]
    fn same_hidden_ignores_walk_ids_order_names_and_unhidden_elements() {
        let mut password = element("e1", "Edit", rect(1, 2, 10, 20));
        password.is_password = true;
        let mut text = child("e2", "e1", "Text");
        let before = snapshot(vec![password.clone(), text.clone()]);
        password.id = "e8".to_owned();
        password.name = "Different private text".to_owned();
        text.id = "e9".to_owned();
        text.parent = Some("e8".to_owned());
        let after = snapshot(vec![
            element("e0", "Button", rect(90, 90, 91, 91)),
            text,
            password,
        ]);
        assert!(same_hidden(&before, &after));
        assert!(same_hidden(&snapshot(vec![]), &snapshot(vec![])));
    }

    #[test]
    fn same_hidden_detects_appearance_removal_motion_resize_and_type_changes() {
        let mut password = element("e1", "Edit", rect(1, 2, 10, 20));
        password.is_password = true;
        let before = snapshot(vec![password.clone()]);
        let empty = snapshot(vec![]);
        assert!(!same_hidden(&before, &empty));
        assert!(!same_hidden(&empty, &before));
        for bounds in [Some(rect(2, 2, 11, 20)), Some(rect(1, 2, 11, 20)), None] {
            let mut changed = password.clone();
            changed.rect = bounds;
            assert!(!same_hidden(&before, &snapshot(vec![changed])));
        }
        let mut changed = password.clone();
        changed.control_type = "Text".to_owned();
        assert!(!same_hidden(&before, &snapshot(vec![changed])));
        let mut duplicate = password.clone();
        duplicate.id = "e2".to_owned();
        assert!(!same_hidden(&before, &snapshot(vec![password, duplicate])));
    }

    #[test]
    fn same_hidden_detects_new_revealed_text_siblings_and_moved_descendants() {
        let root = element("root", "Pane", rect(0, 0, 100, 100));
        let text = child("text", "root", "Text");
        let before = snapshot(vec![root.clone(), text.clone()]);
        let mut toggle = child("toggle", "root", "Button");
        toggle.name = "Show password".to_owned();
        let after = snapshot(vec![root, text, toggle]);
        assert!(!same_hidden(&before, &after));

        let mut password = element("password", "Edit", rect(0, 0, 20, 20));
        password.is_password = true;
        let text = child("text", "password", "Text");
        let before = snapshot(vec![password, text]);
        let mut after = before.clone();
        after.elements[1].rect = Some(rect(1, 1, 11, 11));
        assert!(!same_hidden(&before, &after));
    }

    #[test]
    fn hidden_ids_respects_each_password_flag() {
        let mut password = element("password", "Edit", rect(0, 0, 10, 10));
        password.is_password = true;
        let mut password_like = element("like", "Custom", rect(0, 0, 10, 10));
        password_like.password_like = true;
        assert_eq!(
            hidden(vec![
                password,
                password_like,
                element("plain", "Edit", rect(0, 0, 10, 10)),
            ]),
            ids(&["password", "like"])
        );
    }

    #[test]
    fn hidden_ids_hides_revealed_siblings_for_each_supported_type() {
        for label in ["Show password", "Hide password", "Показать пароль"] {
            let mut toggle = child("toggle", "root", "Button");
            toggle.name = label.to_owned();
            let mut edit = child("edit", "root", "Edit");
            edit.name.clear();
            assert_eq!(
                hidden(vec![
                    element("root", "Window", rect(0, 0, 100, 100)),
                    toggle,
                    edit,
                    child("document", "root", "Document"),
                    child("custom", "root", "Custom"),
                    child("text", "root", "Text"),
                    child("unrelated", "other", "Edit"),
                    element("other", "Pane", rect(0, 0, 100, 100)),
                ]),
                ids(&["edit", "document", "custom", "text"])
            );
        }
    }

    #[test]
    fn a_password_field_or_its_label_is_no_toggle() {
        let mut field = child("field", "root", "Edit");
        field.name = "Password".to_owned();
        field.is_password = true;
        let mut label = child("label", "root", "Text");
        label.name = "Password".to_owned();
        assert_eq!(
            hidden(vec![
                element("root", "Window", rect(0, 0, 100, 100)),
                field,
                label,
                child("username", "root", "Edit"),
                child("hint", "root", "Text"),
            ]),
            ids(&["field"])
        );
    }

    #[test]
    fn hidden_ids_toggle_hides_text_siblings_but_not_buttons() {
        let mut toggle = child("toggle", "root", "Button");
        toggle.name = "Hide password".to_owned();
        toggle.password_like = true;
        assert_eq!(
            hidden(vec![
                element("root", "Window", rect(0, 0, 100, 100)),
                toggle,
                child("text", "root", "Text"),
                child("button", "root", "Button"),
            ]),
            ids(&["toggle", "text"])
        );
    }

    #[test]
    fn hidden_ids_combines_rules_and_hides_deep_descendants_in_any_order() {
        let mut password = child("password", "root", "Custom");
        password.is_password = true;
        let mut toggle = child("toggle", "root", "Button");
        toggle.name = "Show password".to_owned();
        toggle.password_like = true;
        assert_eq!(
            hidden(vec![
                child("deep", "text", "Text"),
                child("text", "password", "Text"),
                child("revealed_text", "revealed", "Text"),
                child("revealed", "root", "Edit"),
                password,
                toggle,
                element("root", "Window", rect(0, 0, 100, 100)),
            ]),
            ids(&[
                "deep",
                "text",
                "password",
                "toggle",
                "revealed",
                "revealed_text"
            ])
        );
    }

    #[test]
    fn hidden_ids_propagates_from_a_flagged_container_without_a_toggle() {
        let mut container = element("container", "Pane", rect(0, 0, 100, 100));
        container.password_like = true;
        assert_eq!(
            hidden(vec![
                container,
                child("child", "container", "Custom"),
                child("text", "child", "Text"),
            ]),
            ids(&["container", "child", "text"])
        );
    }

    #[test]
    fn hidden_ids_parent_cycles_terminate() {
        let mut password = child("password", "text", "Edit");
        password.is_password = true;
        assert_eq!(
            hidden(vec![
                password,
                child("text", "password", "Text"),
                child("unseeded_a", "unseeded_b", "Text"),
                child("unseeded_b", "unseeded_a", "Text"),
                child("self", "self", "Text"),
            ]),
            ids(&["password", "text"])
        );
    }

    #[test]
    fn hidden_ids_ignores_missing_parents_and_separate_roots() {
        let mut toggle = child("toggle", "missing", "Button");
        toggle.name = "Hide password".to_owned();
        let mut root_toggle = element("root_toggle", "Button", rect(0, 0, 10, 10));
        root_toggle.name = "Show password".to_owned();
        let mut orphan = child("orphan", "missing", "Edit");
        orphan.is_password = true;
        assert_eq!(
            hidden(vec![
                toggle,
                child("plain", "missing", "Edit"),
                root_toggle,
                element("root_edit", "Edit", rect(0, 0, 10, 10)),
                orphan,
                child("orphan_child", "orphan", "Text"),
            ]),
            ids(&["orphan", "orphan_child"])
        );
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
