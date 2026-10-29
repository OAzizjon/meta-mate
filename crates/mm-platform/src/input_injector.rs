//! "Do it for me": one injected action, only after a human said yes (invariant 4).

use mm_core::geom::{PhysPoint, PhysRect};
use mm_core::privacy::forbidden;

use crate::error::PlatformError;
use crate::input_hooks::MouseButton;
use crate::target::TargetWindow;

/// One action Mate may perform for the user, in physical virtual-screen pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    Click { at: PhysPoint, button: MouseButton },
    DoubleClick { at: PhysPoint },
}

/// A human's yes for one exact action in one window. Create it only in the handler of the
/// user's own answer to our confirmation prompt, never from model output or
/// `pending_safety_checks`. It is not `Clone`: one yes, one action.
///
/// Created only by answering a platform prompt. The element captured before that prompt
/// is compared with a live read immediately before injection.
#[derive(Debug)]
pub struct HumanConfirmation {
    target: TargetWindow,
    action: InputAction,
    element: ConfirmedElement,
}

impl HumanConfirmation {
    pub fn target(&self) -> TargetWindow {
        self.target
    }

    pub fn action(&self) -> &InputAction {
        &self.action
    }
}

/// A UIA hit-test result, including the safety result of reading its ancestors and owner.
/// Identity uses the exact runtime id, name and type plus a rectangle tolerance of three pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmedElement {
    pub runtime_id: Vec<i32>,
    pub process_id: u32,
    pub name: String,
    pub control_type: String,
    pub rect: PhysRect,
    pub enabled: bool,
    pub offscreen: bool,
    pub blocked: bool,
}

impl ConfirmedElement {
    /// Snapshot identity at the step's click point; runtime identity is checked separately.
    pub fn matches_target(&self, name: &str, control_type: &str, rect: PhysRect) -> bool {
        let close = |a: i32, b: i32| (i64::from(a) - i64::from(b)).abs() <= 3;
        self.name == name
            && self.control_type == control_type
            && close(self.rect.left, rect.left)
            && close(self.rect.top, rect.top)
            && close(self.rect.right, rect.right)
            && close(self.rect.bottom, rect.bottom)
    }
}

/// Only decorative children can use their direct action parent's label.
pub fn can_borrow_click_name(child_type: &str, parent_type: &str) -> bool {
    matches!(child_type, "Image" | "Text" | "Custom")
        && matches!(
            parent_type,
            "Button" | "MenuItem" | "Hyperlink" | "SplitButton" | "TabItem"
        )
}

/// A displayed offer, consumed by one answer from the person's speech handler.
#[derive(Debug)]
pub struct PendingConfirmation(HumanConfirmation);

impl PendingConfirmation {
    /// Never call with model output or a pending safety check.
    pub fn answer(self, yes: bool) -> Option<HumanConfirmation> {
        yes.then_some(self.0)
    }
}

/// The platform presents the prompt itself through the host's localized speech callback.
/// Capture `element` before calling this, never after receiving the person's answer.
pub fn ask_confirmation(
    target: TargetWindow,
    action: &InputAction,
    element: ConfirmedElement,
    show_prompt: impl FnOnce(),
) -> Result<PendingConfirmation, PlatformError> {
    let InputAction::Click {
        at,
        button: MouseButton::Left,
    } = action
    else {
        return Err(PlatformError::SafetyBlocked);
    };
    recheck_element(target, target, *at, &element, &element)?;
    show_prompt();
    Ok(PendingConfirmation(HumanConfirmation {
        target,
        action: action.clone(),
        element,
    }))
}

/// Pure final guard. Unknown UIA properties must fail before constructing a snapshot.
pub fn recheck_element(
    target: TargetWindow,
    foreground: TargetWindow,
    at: PhysPoint,
    confirmed: &ConfirmedElement,
    live: &ConfirmedElement,
) -> Result<(), PlatformError> {
    if foreground != target
        || confirmed.process_id != target.process_id
        || live.process_id != target.process_id
    {
        return Err(PlatformError::WindowNotFound);
    }
    for element in [confirmed, live] {
        if !element.enabled
            || element.runtime_id.is_empty()
            || element.offscreen
            || element.blocked
            || forbidden(&element.name, &element.control_type)
            || !element.rect.contains(at)
        {
            return Err(PlatformError::SafetyBlocked);
        }
    }
    if confirmed.runtime_id != live.runtime_id
        || !confirmed.matches_target(&live.name, &live.control_type, live.rect)
    {
        return Err(PlatformError::SafetyBlocked);
    }
    Ok(())
}

/// Refuses unless `confirmation` is the human's yes for exactly `action` in exactly
/// `target`: a yes in one window never carries over to another (invariant 4). Every
/// `InputInjector` calls this before anything else.
pub fn check_confirmation(
    target: TargetWindow,
    action: &InputAction,
    confirmation: Option<HumanConfirmation>,
) -> Result<ConfirmedElement, PlatformError> {
    match confirmation {
        Some(yes) if yes.target == target && yes.action == *action => Ok(yes.element),
        _ => Err(PlatformError::ConfirmationRequired),
    }
}

/// Injected input into the user's app.
pub trait InputInjector {
    /// Performs `action` after `check_confirmation`, and only while `target` is still the
    /// foreground window and the element under the point is still the confirmed one. The
    /// safety blocklist is checked both before the prompt and immediately before injection.
    fn execute_verified(
        &mut self,
        target: TargetWindow,
        action: &InputAction,
        confirmation: Option<HumanConfirmation>,
    ) -> Result<(), PlatformError>;
}

#[cfg(test)]
fn test_element() -> ConfirmedElement {
    ConfirmedElement {
        runtime_id: vec![1, 42],
        process_id: 42,
        name: "Open".into(),
        control_type: "Button".into(),
        rect: PhysRect::new(0, 0, 40, 40).unwrap(),
        enabled: true,
        offscreen: false,
        blocked: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: TargetWindow = TargetWindow {
        handle: 7,
        process_id: 42,
    };
    const AT: PhysPoint = PhysPoint { x: 20, y: 20 };

    fn check(live: &ConfirmedElement) -> Result<(), PlatformError> {
        recheck_element(TARGET, TARGET, AT, &test_element(), live)
    }

    #[test]
    fn unchanged_element_and_three_pixel_rect_drift_are_allowed() {
        let mut live = test_element();
        assert_eq!(check(&live), Ok(()));
        live.rect = PhysRect::new(3, -3, 43, 37).unwrap();
        assert_eq!(check(&live), Ok(()));
    }

    #[test]
    fn replacement_with_identical_label_and_bounds_is_refused() {
        let mut live = test_element();
        live.runtime_id = vec![1, 43];
        assert_eq!(check(&live), Err(PlatformError::SafetyBlocked));
        live.runtime_id.clear();
        assert_eq!(check(&live), Err(PlatformError::SafetyBlocked));
        assert!(
            ask_confirmation(
                TARGET,
                &InputAction::Click {
                    at: AT,
                    button: MouseButton::Left,
                },
                live,
                || panic!("unknown identity")
            )
            .is_err()
        );
    }

    #[test]
    fn decorative_children_only_borrow_direct_action_parent_names() {
        for child in ["Image", "Text", "Custom"] {
            for parent in ["Button", "MenuItem", "Hyperlink", "SplitButton", "TabItem"] {
                assert!(can_borrow_click_name(child, parent));
            }
            for parent in ["ListItem", "ToolBar", "Pane", "Window"] {
                assert!(!can_borrow_click_name(child, parent));
            }
        }
        assert!(!can_borrow_click_name("Button", "Button"));
    }

    #[test]
    fn changed_name_type_or_any_rect_edge_is_refused() {
        let mut live = test_element();
        live.name = "Close".into();
        assert!(check(&live).is_err());
        live = test_element();
        live.control_type = "MenuItem".into();
        assert!(check(&live).is_err());
        for rect in [
            PhysRect::new(4, 0, 40, 40).unwrap(),
            PhysRect::new(0, 4, 40, 40).unwrap(),
            PhysRect::new(0, 0, 44, 40).unwrap(),
            PhysRect::new(0, 0, 40, 44).unwrap(),
        ] {
            live = test_element();
            live.rect = rect;
            assert!(check(&live).is_err());
        }
    }

    #[test]
    fn changed_foreground_handle_or_process_is_refused() {
        for front in [
            TargetWindow {
                handle: 8,
                ..TARGET
            },
            TargetWindow {
                process_id: 43,
                ..TARGET
            },
        ] {
            assert_eq!(
                recheck_element(TARGET, front, AT, &test_element(), &test_element()),
                Err(PlatformError::WindowNotFound)
            );
        }
        let mut live = test_element();
        live.process_id = 43;
        assert_eq!(check(&live), Err(PlatformError::WindowNotFound));
    }

    #[test]
    fn disabled_offscreen_and_ancestor_blocked_elements_are_refused() {
        for live in [
            ConfirmedElement {
                enabled: false,
                ..test_element()
            },
            ConfirmedElement {
                offscreen: true,
                ..test_element()
            },
            ConfirmedElement {
                blocked: true,
                ..test_element()
            },
        ] {
            assert_eq!(check(&live), Err(PlatformError::SafetyBlocked));
        }
    }

    #[test]
    fn click_point_must_still_be_inside_both_rectangles() {
        let element = test_element();
        assert!(
            recheck_element(
                TARGET,
                TARGET,
                PhysPoint { x: 40, y: 20 },
                &element,
                &element
            )
            .is_err()
        );
        let mut live = element.clone();
        live.rect.left = 21;
        let mut confirmed = live.clone();
        confirmed.rect.left = 18;
        assert!(recheck_element(TARGET, TARGET, AT, &confirmed, &live).is_err());
    }

    #[test]
    fn every_forbidden_category_is_refused_even_with_identical_identity() {
        for (name, kind) in [
            ("Payment", "Button"),
            ("Buy", "Button"),
            ("Delete", "Button"),
            ("Transfer money", "Button"),
            ("Password", "Edit"),
        ] {
            let element = ConfirmedElement {
                name: name.into(),
                control_type: kind.into(),
                ..test_element()
            };
            assert_eq!(
                recheck_element(TARGET, TARGET, AT, &element, &element),
                Err(PlatformError::SafetyBlocked)
            );
        }
    }

    #[test]
    fn platform_prompt_precedes_answer_and_one_yes_is_consumed() {
        let action = InputAction::Click {
            at: AT,
            button: MouseButton::Left,
        };
        let mut shown = false;
        let offer = ask_confirmation(TARGET, &action, test_element(), || shown = true).unwrap();
        assert!(shown);
        let yes = offer.answer(true);
        assert_eq!(check_confirmation(TARGET, &action, yes), Ok(test_element()));
        let no = ask_confirmation(TARGET, &action, test_element(), || {})
            .unwrap()
            .answer(false);
        assert_eq!(
            check_confirmation(TARGET, &action, no),
            Err(PlatformError::ConfirmationRequired)
        );
        let blocked = ConfirmedElement {
            enabled: false,
            ..test_element()
        };
        assert!(ask_confirmation(TARGET, &action, blocked, || panic!("unsafe offer")).is_err());
        assert!(
            ask_confirmation(
                TARGET,
                &InputAction::DoubleClick { at: AT },
                test_element(),
                || panic!("double click")
            )
            .is_err()
        );
    }
}
