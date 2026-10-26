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

/// A human's yes for one exact action. Create it only in the handler of the user's own
/// answer to our confirmation prompt, never from model output or `pending_safety_checks`.
/// It is not `Clone`: one yes, one action.
#[derive(Debug)]
pub struct HumanConfirmation {
    action: InputAction,
}

impl HumanConfirmation {
    pub fn given_by_user(action: InputAction) -> Self {
        Self { action }
    }

    pub fn action(&self) -> &InputAction {
        &self.action
    }
}

/// Refuses unless `confirmation` is the human's yes for exactly `action`. Every
/// `InputInjector` calls this before anything else.
pub fn check_confirmation(
    action: &InputAction,
    confirmation: Option<HumanConfirmation>,
) -> Result<(), PlatformError> {
    match confirmation {
        Some(yes) if yes.action == *action => Ok(()),
        _ => Err(PlatformError::ConfirmationRequired),
    }
}

/// Injected input into the user's app.
pub trait InputInjector {
    /// Performs `action` after `check_confirmation`, and only while `target` is still the
    /// foreground window and the element under the point is still the confirmed one. The
    /// safety blocklist (payment, delete, password) is checked before the user is asked.
    fn execute_verified(
        &mut self,
        target: TargetWindow,
        action: &InputAction,
        confirmation: Option<HumanConfirmation>,
    ) -> Result<(), PlatformError>;
}
