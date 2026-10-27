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

/// A human's yes for one exact action in one window. Create it only in the handler of the
/// user's own answer to our confirmation prompt, never from model output or
/// `pending_safety_checks`. It is not `Clone`: one yes, one action.
///
/// Still open for "do it for me" (Day 7, safety review #460): the constructor should move
/// behind a platform `ask_confirmation(&action, target)` that shows the prompt itself, and
/// the confirmed element (id and rect) should be re-checked right before injecting.
#[derive(Debug)]
pub struct HumanConfirmation {
    target: TargetWindow,
    action: InputAction,
}

impl HumanConfirmation {
    pub fn given_by_user(target: TargetWindow, action: InputAction) -> Self {
        Self { target, action }
    }

    pub fn target(&self) -> TargetWindow {
        self.target
    }

    pub fn action(&self) -> &InputAction {
        &self.action
    }
}

/// Refuses unless `confirmation` is the human's yes for exactly `action` in exactly
/// `target`: a yes in one window never carries over to another (invariant 4). Every
/// `InputInjector` calls this before anything else.
pub fn check_confirmation(
    target: TargetWindow,
    action: &InputAction,
    confirmation: Option<HumanConfirmation>,
) -> Result<(), PlatformError> {
    match confirmation {
        Some(yes) if yes.target == target && yes.action == *action => Ok(()),
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
