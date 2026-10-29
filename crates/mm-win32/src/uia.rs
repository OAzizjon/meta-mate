//! UI Automation snapshots from Rust (spike S3): one cached, bounded walk of a window's
//! control view. Fail closed (invariant 3): any walk error, a hit limit or a skipped
//! offscreen subtree marks the snapshot truncated, and a truncated snapshot blocks the
//! screenshot (`redaction`) unless `sensitive` then finds every field to hide (D50).
//!
//! The walker runs on the calling thread, which `UiaWalker::new` puts into the COM
//! multithreaded apartment; keep it off the UI thread (a hung provider must never
//! freeze Mate).

use mm_core::geom::{PhysPoint, PhysRect};
use mm_core::privacy::password_like;
use mm_platform::{TargetWindow, UiElement, UiSnapshot, is_interactive_type};
use uiautomation::core::UICacheRequest;
use uiautomation::types::{ControlType, Handle, TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::{UIAutomation, UIElement, UITreeWalker};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

/// Limits of one walk. A browser page has thousands of nodes at about 2 ms each, so the
/// walk lists what is on screen (offscreen subtrees are skipped) and stops early; the
/// privacy of a cut walk rests on `sensitive` (D50).
#[derive(Debug, Clone, Copy)]
pub struct WalkLimits {
    pub max_nodes: usize,
    pub max_depth: usize,
}

impl Default for WalkLimits {
    fn default() -> Self {
        Self {
            max_nodes: 500,
            max_depth: 32,
        }
    }
}

/// Every property a snapshot reads, fetched in one round trip per element.
const PROPERTIES: [UIProperty; 9] = [
    UIProperty::ControlType,
    UIProperty::Name,
    UIProperty::AutomationId,
    UIProperty::ClassName,
    UIProperty::HelpText,
    UIProperty::BoundingRectangle,
    UIProperty::IsEnabled,
    UIProperty::IsOffscreen,
    UIProperty::IsPassword,
];

/// Control types a password field, a PIN box or a show/hide-password toggle comes as
/// (what `password_like` and `UiSnapshot::hidden_ids` look at); `IsPassword` is asked for
/// separately. Text labels and plain inputs are retained for contextual classification.
const SENSITIVE_TYPES: [ControlType; 9] = [
    ControlType::Edit,
    ControlType::ComboBox,
    ControlType::Text,
    ControlType::Document,
    ControlType::Custom,
    ControlType::Button,
    ControlType::CheckBox,
    ControlType::Hyperlink,
    ControlType::Image,
];

pub struct UiaWalker {
    automation: UIAutomation,
    cache: UICacheRequest,
    walker: UITreeWalker,
}

impl UiaWalker {
    /// A live hit with the walk's cached properties; COM stays on the calling thread.
    pub fn element_at_point(&self, point: PhysPoint) -> Option<UiElement> {
        let element = self
            .automation
            .element_from_point_build_cache(
                uiautomation::types::Point::new(point.x, point.y),
                &self.cache,
            )
            .ok()?;
        describe(&element, "p0", None)
    }

    /// Initializes COM (multithreaded) on this thread and prepares the cache request.
    pub fn new() -> uiautomation::Result<Self> {
        let automation = UIAutomation::new()?;
        let cache = automation.create_cache_request()?;
        for property in PROPERTIES {
            cache.add_property(property)?;
        }
        cache.set_tree_scope(TreeScope::Element)?;
        let walker = automation.get_control_view_walker()?;
        Ok(Self {
            automation,
            cache,
            walker,
        })
    }

    /// A snapshot of `hwnd`'s control view, pre-order, ids `e0`, `e1`, ... in walk
    /// order. Never reads control values (passwords stay where they are).
    pub fn snapshot(&self, hwnd: HWND, limits: WalkLimits) -> uiautomation::Result<UiSnapshot> {
        let mut elements = Vec::new();
        let truncated = self.walk(hwnd, &mut elements, limits)?;
        let frame = elements
            .first()
            .and_then(|root: &UiElement| root.rect)
            .unwrap_or(PhysRect {
                left: 0,
                top: 0,
                right: 1,
                bottom: 1,
            });
        Ok(UiSnapshot {
            target: target_window(hwnd),
            frame,
            elements,
            truncated,
        })
    }

    /// The window and its owned popups (menus, dropdowns) as one snapshot of `region`, the
    /// area that is captured: ids run on across the roots and the node budget is shared. A
    /// popup that cannot be walked marks the snapshot truncated, so redaction fails closed.
    pub fn snapshot_with_popups(
        &self,
        hwnd: HWND,
        popups: &[HWND],
        region: PhysRect,
        limits: WalkLimits,
    ) -> uiautomation::Result<UiSnapshot> {
        let mut elements = Vec::new();
        let mut truncated = self.walk(hwnd, &mut elements, limits)?;
        for &popup in popups {
            truncated |= self.walk(popup, &mut elements, limits)?;
        }
        Ok(UiSnapshot {
            target: target_window(hwnd),
            frame: region,
            elements,
            truncated,
        })
    }

    /// Every element under `hwnd` that could be a password, a PIN box or a reveal toggle,
    /// found by the provider itself in one FindAll (no round trip per node, so it covers
    /// pages far larger than a walk can), with the walk's cached properties. Ids `s0`,
    /// `s1`, ...; parents unknown. None when the search or any element's properties fail:
    /// a partial list cannot vouch for the picture (invariant 3).
    pub fn sensitive(&self, hwnd: HWND) -> Option<Vec<UiElement>> {
        let automation = &self.automation;
        let root = automation
            .element_from_handle_build_cache(Handle::from(hwnd), &self.cache)
            .ok()?;
        let mut condition = automation
            .create_property_condition(UIProperty::IsPassword, Variant::from(true), None)
            .ok()?;
        for kind in SENSITIVE_TYPES {
            let by_type = automation
                .create_property_condition(
                    UIProperty::ControlType,
                    Variant::from(kind as i32),
                    None,
                )
                .ok()?;
            condition = automation.create_or_condition(condition, by_type).ok()?;
        }
        // Only what is on screen can be in the picture (Chromium marks everything scrolled
        // out of view offscreen: thousands of links on a long page).
        let on_screen = automation
            .create_property_condition(UIProperty::IsOffscreen, Variant::from(false), None)
            .ok()?;
        let condition = automation
            .create_and_condition(on_screen.clone(), condition)
            .ok()?;
        let found = root
            .find_all_build_cache(TreeScope::Subtree, &condition, &self.cache)
            .ok()?;
        // Keep plain inputs and separate labels until contextual classification is done.
        // A cut walk may have seen neither; dropping them would falsely certify it.
        let mut candidates = UiSnapshot {
            target: target_window(hwnd),
            frame: describe(&root, "root", None)?.rect?,
            elements: found
                .iter()
                .enumerate()
                .map(|(i, element)| describe(element, &format!("c{i}"), None))
                .collect::<Option<Vec<_>>>()?,
            truncated: false,
        };
        mark_contextual_inputs(&mut candidates);
        let mut hide = Vec::new();
        for (element, candidate) in found.iter().zip(&candidates.elements) {
            // Parent links are absent from FindAll. Search the label's surrounding
            // subtree as well, covering siblings with distant or missing bounds.
            if candidate.control_type == "Text" && candidate.private_label() {
                self.siblings_of(element, &on_screen, &mut hide)?;
            }
            if !(candidate.is_password || candidate.password_like) {
                continue;
            }
            let mut described = candidate.clone();
            described.id = format!("s{}", hide.len());
            described.password_like = true;
            // A show/hide-password toggle: the field beside it may hold the password in
            // plain text (D37), whatever it is called.
            let toggle = !matches!(described.control_type.as_str(), "Edit" | "Document")
                && !described.is_password;
            hide.push(described);
            self.descendants_of(element, &on_screen, &mut hide)?;
            if toggle {
                self.siblings_of(element, &on_screen, &mut hide)?;
            }
        }
        Some(hide)
    }

    /// Everything on screen inside `field` (a text run may stick out of its box), marked
    /// password-like.
    fn descendants_of(
        &self,
        field: &UIElement,
        on_screen: &uiautomation::core::UICondition,
        hide: &mut Vec<UiElement>,
    ) -> Option<()> {
        let inside = field
            .find_all_build_cache(TreeScope::Descendants, on_screen, &self.cache)
            .ok()?;
        for element in &inside {
            let mut described = describe(element, &format!("s{}", hide.len()), None)?;
            described.password_like = true;
            hide.push(described);
        }
        Some(())
    }

    /// The fields near `toggle` (every on-screen Edit, Document, Custom and Text under the
    /// nearest ancestor that holds an edit field, so their insides too), marked
    /// password-like.
    fn siblings_of(
        &self,
        toggle: &UIElement,
        on_screen: &uiautomation::core::UICondition,
        hide: &mut Vec<UiElement>,
    ) -> Option<()> {
        // Web forms wrap the field and its toggle in separate boxes: climb from the toggle
        // until an ancestor holds an edit field (at most four levels), and hide what that
        // ancestor holds. Hiding a little too much is safe; missing the field is not.
        let automation = &self.automation;
        let mut fields = automation
            .create_property_condition(
                UIProperty::ControlType,
                Variant::from(ControlType::Edit as i32),
                None,
            )
            .ok()?;
        for kind in [
            ControlType::ComboBox,
            ControlType::Document,
            ControlType::Custom,
            ControlType::Text,
        ] {
            let by_type = automation
                .create_property_condition(
                    UIProperty::ControlType,
                    Variant::from(kind as i32),
                    None,
                )
                .ok()?;
            fields = automation.create_or_condition(fields, by_type).ok()?;
        }
        let fields = automation
            .create_and_condition(on_screen.clone(), fields)
            .ok()?;
        let mut area = toggle.clone();
        let mut near = Vec::new();
        for _ in 0..4 {
            // An unreadable ancestor cannot certify contextual coverage.
            let up = self
                .walker
                .get_parent_build_cache(&area, &self.cache)
                .ok()?;
            area = up;
            near = area
                .find_all_build_cache(TreeScope::Descendants, &fields, &self.cache)
                .ok()?;
            let has_edit = near.iter().any(|element| {
                element
                    .get_cached_control_type()
                    .is_ok_and(|kind| matches!(kind, ControlType::Edit | ControlType::ComboBox))
            });
            if has_edit {
                break;
            }
        }
        for element in &near {
            let mut described = describe(element, &format!("s{}", hide.len()), None)?;
            described.password_like = true;
            hide.push(described);
        }
        Some(())
    }

    /// Walks `hwnd`'s control view onto `elements` (ids continue from its length); true when
    /// the walk was cut short.
    fn walk(
        &self,
        hwnd: HWND,
        elements: &mut Vec<UiElement>,
        limits: WalkLimits,
    ) -> uiautomation::Result<bool> {
        let root = self
            .automation
            .element_from_handle_build_cache(Handle::from(hwnd), &self.cache)?;
        let mut truncated = false;
        // (element, parent id, depth)
        let mut stack: Vec<(UIElement, Option<String>, usize)> = vec![(root, None, 0)];
        'walk: while let Some((element, parent, depth)) = stack.pop() {
            if elements.len() >= limits.max_nodes {
                truncated = true;
                break;
            }
            let id = format!("e{}", elements.len());
            // Unlike a node/depth limit, an unreadable password flag cannot be
            // repaired by the later sensitive-control search.
            element.is_cached_password()?;
            let offscreen = match describe(&element, &id, parent) {
                Some(described) => {
                    let offscreen = described.offscreen;
                    elements.push(described);
                    offscreen
                }
                None => {
                    truncated = true;
                    continue;
                }
            };
            // Not on screen, so not in the picture: its subtree is skipped (a long page
            // keeps thousands of nodes there). The walk then vouches for nothing.
            if depth > 0 && offscreen {
                truncated = true;
                continue;
            }
            if depth + 1 > limits.max_depth {
                truncated |= match self
                    .walker
                    .get_first_child_build_cache(&element, &self.cache)
                {
                    Ok(_) => true,
                    Err(error) => !is_end(error.code()),
                };
                continue;
            }
            // Children are pushed in reverse so they pop in document order.
            let mut children = Vec::new();
            let mut next = self
                .walker
                .get_first_child_build_cache(&element, &self.cache);
            loop {
                let child = match next {
                    Ok(child) => child,
                    Err(error) if is_end(error.code()) => break,
                    Err(_) => {
                        truncated = true;
                        continue 'walk;
                    }
                };
                next = self
                    .walker
                    .get_next_sibling_build_cache(&child, &self.cache);
                children.push(child);
                if children.len() > limits.max_nodes {
                    truncated = true;
                    break;
                }
            }
            for child in children.into_iter().rev() {
                stack.push((child, Some(id.clone()), depth + 1));
            }
        }
        Ok(truncated)
    }
}

/// A successful COM call with a null element means the traversal has ended.
fn is_end(code: i32) -> bool {
    code == 0
}

/// Classify before filtering: a provider-found plain input may be a revealed secret.
fn mark_contextual_inputs(candidates: &mut UiSnapshot) {
    let ambiguous = candidates.ambiguous_private_edits();
    for element in &mut candidates.elements {
        if ambiguous.contains(&element.id) {
            element.password_like = true;
        }
    }
}

struct PropertyFlags {
    password_like: bool,
    is_password: bool,
    enabled: bool,
    offscreen: bool,
}

/// Missing text may conceal a password signal; unknown visibility stays on screen.
fn property_flags(
    control_type: &str,
    texts: [Option<&str>; 4],
    is_password: Option<bool>,
    enabled: Option<bool>,
    offscreen: Option<bool>,
) -> Option<PropertyFlags> {
    let is_password = is_password?;
    let password_like = match texts {
        [
            Some(name),
            Some(automation_id),
            Some(class_name),
            Some(help_text),
        ] => password_like(control_type, name, automation_id, help_text, class_name),
        _ => true,
    };
    Some(PropertyFlags {
        password_like,
        is_password,
        enabled: enabled.unwrap_or(false),
        offscreen: offscreen.unwrap_or(false),
    })
}

/// One element's cached properties; `None` when the provider failed to answer.
fn describe(element: &UIElement, id: &str, parent: Option<String>) -> Option<UiElement> {
    let control_type = format!("{:?}", element.get_cached_control_type().ok()?);
    let name = element.get_cached_name();
    let automation_id = element.get_cached_automation_id();
    let class_name = element.get_cached_classname();
    let help_text = element.get_cached_help_text();
    let rect = element
        .get_cached_bounding_rectangle()
        .ok()
        .and_then(|r| PhysRect::new(r.get_left(), r.get_top(), r.get_right(), r.get_bottom()).ok());
    let flags = property_flags(
        &control_type,
        [
            name.as_deref().ok(),
            automation_id.as_deref().ok(),
            class_name.as_deref().ok(),
            help_text.as_deref().ok(),
        ],
        // A failed read invalidates this description without inventing a password.
        element.is_cached_password().ok(),
        element.is_cached_enabled().ok(),
        element.is_cached_offscreen().ok(),
    )?;
    Some(UiElement {
        id: id.to_owned(),
        parent,
        interactive: is_interactive_type(&control_type),
        password_like: flags.password_like,
        control_type,
        name: name.unwrap_or_default(),
        rect,
        enabled: flags.enabled,
        offscreen: flags.offscreen,
        is_password: flags.is_password,
    })
}

fn target_window(hwnd: HWND) -> TargetWindow {
    let mut process_id = 0u32;
    // SAFETY: the out pointer is a local u32 that outlives the call.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    TargetWindow {
        handle: hwnd.0 as usize as u64,
        process_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_scan_keeps_labels_and_classifies_plain_inputs_past_walk_limit() {
        assert!(SENSITIVE_TYPES.contains(&ControlType::Text));
        for kind in [
            ControlType::Edit,
            ControlType::ComboBox,
            ControlType::Custom,
        ] {
            assert!(SENSITIVE_TYPES.contains(&kind));
            let bounds = PhysRect::new(0, 0, 100, 30).unwrap();
            let text = |id: String, name: &str| UiElement {
                id,
                parent: None,
                control_type: "Text".into(),
                name: name.into(),
                rect: Some(bounds),
                interactive: false,
                enabled: true,
                offscreen: false,
                is_password: false,
                password_like: false,
            };
            let mut elements: Vec<_> = (0..501)
                .map(|i| text(format!("c{i}"), "Article paragraph"))
                .collect();
            let mut input = text("input".into(), "Enter here");
            input.control_type = format!("{kind:?}");
            elements.push(input);
            elements.push(text("label".into(), "Password"));
            let mut candidates = UiSnapshot {
                target: TargetWindow {
                    handle: 1,
                    process_id: 1,
                },
                frame: bounds,
                elements,
                truncated: false,
            };
            mark_contextual_inputs(&mut candidates);
            assert!(candidates.element("input").unwrap().password_like);
            assert!(!candidates.element("c0").unwrap().password_like);
        }
    }

    #[test]
    fn only_s_ok_is_end() {
        assert!(is_end(0));
        for code in [
            1,
            0x80040201u32 as i32,
            0x80131505u32 as i32,
            0x800706BAu32 as i32,
        ] {
            assert!(!is_end(code));
        }
    }

    #[test]
    fn each_failed_text_read_is_password_like() {
        for index in 0..4 {
            let mut texts = [Some(""); 4];
            texts[index] = None;
            let flags =
                property_flags("Text", texts, Some(false), Some(true), Some(false)).unwrap();
            assert!(flags.password_like, "failed text read {index}");
        }
    }

    #[test]
    fn successful_text_reads_keep_password_heuristics() {
        let plain = property_flags("Edit", [Some(""); 4], Some(false), None, None).unwrap();
        assert!(!plain.password_like);
        for index in 0..4 {
            let mut texts = [Some(""); 4];
            texts[index] = Some(if index == 2 {
                "PasswordBox"
            } else {
                "Password"
            });
            assert!(
                property_flags("Edit", texts, Some(false), None, None)
                    .unwrap()
                    .password_like
            );
        }
    }

    #[test]
    fn failed_flags_fail_closed() {
        assert!(property_flags("Edit", [Some(""); 4], None, None, None).is_none());
        let flags = property_flags("Edit", [Some(""); 4], Some(false), None, None).unwrap();
        assert!(!flags.is_password);
        assert!(!flags.enabled);
        assert!(!flags.offscreen);
    }

    #[test]
    fn successful_flags_keep_both_values() {
        for value in [false, true] {
            let flags =
                property_flags("Edit", [Some(""); 4], Some(value), Some(value), Some(value))
                    .unwrap();
            assert_eq!(flags.is_password, value);
            assert_eq!(flags.enabled, value);
            assert_eq!(flags.offscreen, value);
        }
    }
}
