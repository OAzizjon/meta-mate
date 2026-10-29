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

fn private_phrase(name: &str) -> bool {
    let name = name.to_lowercase();
    [
        "login",
        "log in",
        "sign in to",
        "password",
        "пароль",
        "вход",
        "payment",
        "pay now",
        "card number",
        "cvv",
        "вход в аккаунт",
        "войти в",
        "оплата",
        "номер карты",
    ]
    .iter()
    .any(|phrase| {
        name.match_indices(phrase).any(|(start, matched)| {
            let end = start + matched.len();
            !name[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
                && !name[end..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    })
}

fn nearby(a: PhysRect, b: PhysRect) -> bool {
    let dx = (i64::from(a.left) - i64::from(b.right))
        .max(i64::from(b.left) - i64::from(a.right))
        .max(0);
    let dy = (i64::from(a.top) - i64::from(b.bottom))
        .max(i64::from(b.top) - i64::from(a.bottom))
        .max(0);
    dx <= 200 && dy <= 200 && dx * dx + dy * dy <= 200 * 200
}

impl UiSnapshot {
    /// Either the window is opaque or private edits need conservative masking.
    pub fn unclassifiable(&self) -> bool {
        self.flat() || !self.ambiguous_private_edits().is_empty()
    }

    /// Unnamed edits beside private labels must be masked, regardless of consent.
    pub fn ambiguous_private_edits(&self) -> BTreeSet<String> {
        self.elements
            .iter()
            .filter(|edit| {
                edit.control_type == "Edit"
                    && edit.name.trim().is_empty()
                    && self.elements.iter().any(|phrase| {
                        private_phrase(&phrase.name)
                            && ((edit.parent.is_some() && edit.parent == phrase.parent)
                                || edit
                                    .rect
                                    .zip(phrase.rect)
                                    .is_some_and(|(a, b)| nearby(a, b)))
                    })
            })
            .map(|edit| edit.id.clone())
            .collect()
    }

    /// Browser chrome is never evidence that a canvas page can be inspected.
    pub fn flat(&self) -> bool {
        let mut chrome: BTreeSet<&str> = self
            .elements
            .iter()
            .filter(|e| e.control_type == "TitleBar")
            .map(|e| e.id.as_str())
            .collect();
        // Exclude all title-bar descendants, including nested system buttons.
        loop {
            let before = chrome.len();
            for e in &self.elements {
                if e.parent.as_deref().is_some_and(|p| chrome.contains(p)) {
                    chrome.insert(&e.id);
                }
            }
            if chrome.len() == before {
                break;
            }
        }
        let parents: BTreeSet<&str> = self
            .elements
            .iter()
            .filter_map(|e| e.parent.as_deref())
            .collect();
        // Only a visible page-sized document scopes the content walk. Native apps can
        // expose small rich-edit documents or hidden help panes beside their controls.
        let documents: BTreeSet<&str> = self
            .elements
            .iter()
            .filter(|e| {
                e.control_type == "Document"
                    && !e.offscreen
                    && e.rect.is_some_and(|r| mostly_inside(self.frame, r))
            })
            .map(|e| e.id.as_str())
            .collect();
        let mut page = documents.clone();
        loop {
            let before = page.len();
            for e in &self.elements {
                if e.parent.as_deref().is_some_and(|p| page.contains(p)) {
                    page.insert(&e.id);
                }
            }
            if page.len() == before {
                break;
            }
        }
        let readable = |e: &&UiElement| {
            !e.offscreen
                && !chrome.contains(e.id.as_str())
                && (documents.is_empty() || page.contains(e.id.as_str()))
                && !matches!(
                    e.control_type.as_str(),
                    "Window" | "Document" | "Custom" | "Image"
                )
                && (e.control_type != "Pane" || parents.contains(e.id.as_str()))
        };
        let content = self.elements.iter().filter(readable).count();
        let reads_content = self.elements.iter().any(|e| {
            readable(&e)
                && ((e.control_type == "Edit" && content >= 3)
                    || (!documents.is_empty()
                        && matches!(e.control_type.as_str(), "Text" | "Hyperlink" | "Heading")
                        && !e.name.trim().is_empty()))
        });
        // A large opaque surface can sit beside a perfectly accessible toolbar.
        let opaque = self.elements.iter().any(|e| {
            !e.offscreen
                && matches!(e.control_type.as_str(), "Custom" | "Pane" | "Image")
                && !parents.contains(e.id.as_str())
                && e.rect.is_some_and(|r| mostly_inside(self.frame, r))
        });
        opaque || (content < 8 && !reads_content)
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

        let ambiguous = self.ambiguous_private_edits();
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
                || toggle_sibling
                || ambiguous.contains(&element.id))
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
    fn sparse_tree_and_unlabelled_private_edit_are_separate_classification_risks() {
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
        assert!(snapshot.unclassifiable());
        for (index, e) in snapshot.elements.iter_mut().enumerate() {
            e.id = format!("e{index}");
            e.control_type = "Text".into();
        }
        assert!(!snapshot.unclassifiable());
        snapshot.elements[0].control_type = "Edit".into();
        snapshot.elements[0].name.clear();
        for title in [
            "Login",
            "Log in",
            "Sign in to your account",
            "Password",
            "Пароль",
            "Вход",
            "Payment",
            "Pay now",
            "Card number",
            "CVV",
            "Вход в аккаунт",
            "Войти в аккаунт",
            "Оплата",
            "Номер карты",
        ] {
            let mut root = snapshot.elements[0].clone();
            root.control_type = "Window".into();
            root.name = title.into();
            snapshot.elements.push(root);
            assert!(snapshot.unclassifiable(), "{title}");
            snapshot.elements.pop();
        }
        for title in [
            "Discard",
            "SD card",
            "Карты",
            "Картинки",
            "Входящие",
            "Sign-in options",
            "Sign in",
            "repayment",
            "log inside",
            "cvvalue",
        ] {
            let mut label = snapshot.elements[1].clone();
            label.name = title.into();
            snapshot.elements.push(label);
            assert!(!snapshot.unclassifiable(), "{title}");
            snapshot.elements.pop();
        }
        let mut label = snapshot.elements[1].clone();
        label.name = "Payment".into();
        label.rect = Some(rect(1000, 1000, 1100, 1050));
        label.parent = Some("other".into());
        snapshot.elements.push(label);
        assert!(!snapshot.unclassifiable());
        snapshot.elements[0].parent = Some("other".into());
        assert!(snapshot.unclassifiable());
        snapshot.elements[0].parent = Some("form".into());
        snapshot.elements.last_mut().unwrap().rect = Some(rect(300, 0, 400, 100));
        assert!(snapshot.unclassifiable());
        snapshot.elements.last_mut().unwrap().rect = Some(rect(301, 0, 400, 100));
        assert!(!snapshot.unclassifiable());
    }

    #[test]
    fn flatness_counts_content_without_window_chrome_or_empty_panes() {
        let mut nodes = vec![
            element("root", "Window", rect(0, 0, 100, 100)),
            child("title", "root", "TitleBar"),
            child("close", "title", "Button"),
            child("pane", "root", "Pane"),
        ];
        for i in 0..7 {
            nodes.push(child(&format!("c{i}"), "root", "Text"));
        }
        let mut snapshot = snapshot(nodes);
        assert!(snapshot.unclassifiable());
        snapshot.elements.push(child("c7", "root", "Text"));
        assert!(!snapshot.unclassifiable());
        snapshot.elements.pop();
        snapshot.elements.last_mut().unwrap().parent = Some("pane".into());
        assert!(!snapshot.unclassifiable());
        for kind in ["Edit", "Document", "Custom"] {
            snapshot.elements = vec![element("one", kind, rect(0, 0, 100, 100))];
            assert!(snapshot.unclassifiable());
        }
    }

    #[test]
    fn ambiguous_private_edits_and_their_descendants_are_hidden_even_without_bounds() {
        let mut label = child("label", "document", "Text");
        label.name = "Password".into();
        let mut edit = child("edit", "document", "Edit");
        edit.name.clear();
        let mut page = snapshot(vec![
            element("root", "Window", rect(0, 0, 100, 100)),
            element("document", "Document", rect(0, 0, 100, 100)),
            label,
            edit,
            child("revealed", "edit", "Text"),
        ]);
        assert_eq!(page.ambiguous_private_edits(), ids(&["edit"]));
        assert!(page.hidden_ids().contains("edit"));
        assert!(page.hidden_ids().contains("revealed"));
        page.elements[3].rect = None;
        assert_eq!(page.ambiguous_private_edits(), ids(&["edit"]));
        assert!(page.hidden_ids().contains("edit"));
        assert!(page.hidden_ids().contains("revealed"));
    }

    #[test]
    fn short_documents_with_real_text_are_readable() {
        for kind in ["Text", "Hyperlink", "Heading"] {
            let mut page = snapshot(vec![
                element("root", "Window", rect(0, 0, 100, 100)),
                element("document", "Document", rect(0, 0, 100, 100)),
                child("heading", "document", kind),
                child("paragraph1", "document", "Text"),
                child("paragraph2", "document", "Text"),
            ]);
            page.elements[1].parent = Some("root".into());
            assert!(!page.flat(), "short article with {kind}");
            for element in &mut page.elements[2..] {
                element.name = "   ".into();
            }
            assert!(page.flat(), "unnamed nodes are not readable text");
            page.elements[2].name = "Article heading".into();
            page.elements[2].offscreen = true;
            assert!(page.flat(), "offscreen text is not evidence");
        }
    }

    #[test]
    fn empty_and_canvas_only_documents_are_flat() {
        let mut page = snapshot(vec![
            element("root", "Window", rect(0, 0, 100, 100)),
            element("document", "Document", rect(0, 0, 100, 100)),
        ]);
        page.elements[1].parent = Some("root".into());
        assert!(page.flat(), "empty document");
        for kind in ["Custom", "Image", "Pane"] {
            let mut canvas = child("canvas", "document", kind);
            canvas.rect = Some(page.frame);
            page.elements.push(canvas);
            assert!(page.flat(), "document containing a large {kind}");
            page.elements.pop();
        }
    }

    #[test]
    fn browser_chrome_cannot_vouch_for_canvas_or_remote_page() {
        let mut nodes = vec![
            element("root", "Window", rect(0, 0, 100, 100)),
            child("address", "root", "Edit"),
            child("document", "root", "Document"),
            child("canvas", "document", "Custom"),
        ];
        nodes[2].rect = Some(rect(0, 10, 100, 100));
        for i in 0..12 {
            nodes.push(child(&format!("toolbar{i}"), "root", "Button"));
        }
        let mut page = snapshot(nodes);
        assert!(page.flat());
        for i in 0..8 {
            page.elements
                .push(child(&format!("text{i}"), "document", "Text"));
        }
        assert!(!page.flat(), "normal accessible Edge page");
        page.elements
            .iter_mut()
            .find(|e| e.id == "canvas")
            .unwrap()
            .rect = Some(page.frame);
        assert!(
            page.flat(),
            "opaque remote surface beside accessible controls"
        );
    }

    #[test]
    fn ordinary_settings_and_local_edits_are_readable() {
        for title in ["Settings", "Notepad", "Telegram"] {
            let mut nodes = vec![element("root", "Window", rect(0, 0, 100, 100))];
            nodes[0].name = title.into();
            nodes.extend([
                child("edit", "root", "Edit"),
                child("label", "root", "Text"),
                child("save", "root", "Button"),
            ]);
            let mut app = snapshot(nodes);
            assert!(!app.unclassifiable(), "{title}");
            let mut help = child("help", "root", "Document");
            help.rect = Some(app.frame);
            help.offscreen = true;
            app.elements.push(help);
            assert!(!app.unclassifiable(), "{title}: hidden help document");
            let help = app.elements.last_mut().unwrap();
            help.offscreen = false;
            help.rect = Some(rect(10, 10, 20, 20));
            assert!(!app.unclassifiable(), "{title}: small rich-edit document");
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
