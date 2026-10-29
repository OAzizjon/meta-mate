//! "Do it for me": one injected action, only after a human said yes (invariant 4).

use mm_core::geom::PhysPoint;

use crate::error::PlatformError;
use crate::input_hooks::MouseButton;
use crate::target::TargetWindow;

/// One action Mate may perform for the user, in physical virtual-screen pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    Click { at: PhysPoint, button: MouseButton },
    DoubleClick { at: PhysPoint },
}

/// Only decorative children can use their direct action parent's label.
pub fn can_borrow_click_name(child_type: &str, parent_type: &str) -> bool {
    matches!(child_type, "Image" | "Text" | "Custom")
        && matches!(
            parent_type,
            "Button" | "MenuItem" | "Hyperlink" | "SplitButton" | "TabItem"
        )
}

/// Injected input into the user's app. Each backend owns its confirmation capability.
pub trait InputInjector {
    type Confirmation;
    type Offer;

    /// Backends without a live desktop cannot issue confirmations. In particular,
    /// overriding another method cannot cause this default to mint a capability.
    fn ask_confirmation(
        &mut self,
        _target: TargetWindow,
        _action: &InputAction,
    ) -> Result<Self::Offer, PlatformError> {
        Err(PlatformError::SafetyBlocked)
    }

    /// Consume one backend-owned confirmation for exactly this target and action.
    fn execute_verified(
        &mut self,
        target: TargetWindow,
        action: &InputAction,
        confirmation: Option<Self::Confirmation>,
    ) -> Result<(), PlatformError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OtherInjector;

    impl InputInjector for OtherInjector {
        type Confirmation = ();
        type Offer = ();

        fn execute_verified(
            &mut self,
            _target: TargetWindow,
            _action: &InputAction,
            _confirmation: Option<()>,
        ) -> Result<(), PlatformError> {
            Err(PlatformError::SafetyBlocked)
        }
    }

    #[test]
    fn trait_default_cannot_mint_an_offer_for_another_injector() {
        let target = TargetWindow {
            handle: 7,
            process_id: 42,
        };
        let action = InputAction::Click {
            at: PhysPoint { x: 20, y: 20 },
            button: MouseButton::Left,
        };
        assert_eq!(
            OtherInjector.ask_confirmation(target, &action),
            Err(PlatformError::SafetyBlocked)
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
}
