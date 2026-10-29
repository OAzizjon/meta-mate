//! In-memory backend for unit tests: fill the public fields, run the code under test, then
//! read what it did.

use std::collections::VecDeque;

use mm_core::geom::{FrameTransform, PhysPoint, PhysRect};

use crate::captured_image::CapturedImage;
use crate::error::PlatformError;
use crate::input_hooks::{InputEvent, InputHooks};
use crate::input_injector::{HumanConfirmation, InputAction, InputInjector, check_confirmation};
use crate::overlay::{Overlay, RobotPose};
use crate::screen_capture::ScreenCapture;
use crate::target::TargetWindow;
use crate::ui_snapshot::{UiElement, UiSnapshot};
use crate::ui_tree::UiTree;

/// Implements every platform trait from memory. Its only window is `snapshot.target`.
pub struct MockBackend {
    /// What `UiTree` serves.
    pub snapshot: UiSnapshot,
    /// Returned in order by `poll_event`.
    pub events: VecDeque<InputEvent>,
    /// One per `capture_target`; an empty queue is `CaptureFailed`.
    pub frames: VecDeque<(CapturedImage, FrameTransform)>,
    /// The last pose set through `Overlay`.
    pub robot_pose: Option<RobotPose>,
    /// The last highlight set through `Overlay`.
    pub highlight: Option<PhysRect>,
    pub hidden_for_capture: bool,
    /// Actions that passed the confirmation and target checks, in order.
    pub executed: Vec<(TargetWindow, InputAction)>,
}

impl MockBackend {
    pub fn new(snapshot: UiSnapshot) -> Self {
        Self {
            snapshot,
            events: VecDeque::new(),
            frames: VecDeque::new(),
            robot_pose: None,
            highlight: None,
            hidden_for_capture: false,
            executed: Vec::new(),
        }
    }

    fn check_target(&self, target: TargetWindow) -> Result<(), PlatformError> {
        if target == self.snapshot.target {
            Ok(())
        } else {
            Err(PlatformError::WindowNotFound)
        }
    }
}

impl UiTree for MockBackend {
    fn snapshot(&self, target: TargetWindow) -> Result<UiSnapshot, PlatformError> {
        self.check_target(target)?;
        Ok(self.snapshot.clone())
    }

    fn element_from_point(&self, point: PhysPoint) -> Result<Option<UiElement>, PlatformError> {
        Ok(self.snapshot.target_at(point).cloned())
    }
}

impl InputHooks for MockBackend {
    fn poll_event(&mut self) -> Option<InputEvent> {
        self.events.pop_front()
    }
}

impl ScreenCapture for MockBackend {
    fn capture_target(
        &mut self,
        target: TargetWindow,
    ) -> Result<(CapturedImage, FrameTransform), PlatformError> {
        self.check_target(target)?;
        self.frames.pop_front().ok_or(PlatformError::CaptureFailed)
    }
}

impl Overlay for MockBackend {
    fn set_robot_pose(&mut self, pose: RobotPose) -> Result<(), PlatformError> {
        self.robot_pose = Some(pose);
        Ok(())
    }

    fn set_highlight(&mut self, rect: Option<PhysRect>) -> Result<(), PlatformError> {
        self.highlight = rect;
        Ok(())
    }

    fn set_hidden_for_capture(&mut self, hidden: bool) -> Result<(), PlatformError> {
        self.hidden_for_capture = hidden;
        Ok(())
    }
}

impl InputInjector for MockBackend {
    fn execute_verified(
        &mut self,
        target: TargetWindow,
        action: &InputAction,
        confirmation: Option<HumanConfirmation>,
    ) -> Result<(), PlatformError> {
        check_confirmation(target, action, confirmation)?;
        self.check_target(target)?;
        self.executed.push((target, action.clone()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_hooks::MouseButton;

    fn backend() -> MockBackend {
        MockBackend::new(UiSnapshot {
            target: TargetWindow {
                handle: 7,
                process_id: 42,
            },
            frame: PhysRect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            elements: Vec::new(),
            truncated: false,
        })
    }

    #[test]
    fn injection_needs_the_humans_yes_for_that_action() {
        let mut mock = backend();
        let target = mock.snapshot.target;
        let at = PhysPoint { x: 5, y: 5 };
        let click = InputAction::Click {
            at,
            button: MouseButton::Left,
        };
        let refused = Err(PlatformError::ConfirmationRequired);
        assert_eq!(mock.execute_verified(target, &click, None), refused);
        let other = confirmed(
            target,
            InputAction::Click {
                at: PhysPoint { x: 6, y: 5 },
                button: MouseButton::Left,
            },
        );
        assert_eq!(mock.execute_verified(target, &click, Some(other)), refused);
        // A yes given in another window never carries over (invariant 4).
        let elsewhere = TargetWindow {
            handle: 8,
            process_id: 42,
        };
        let wrong_window = confirmed(elsewhere, click.clone());
        assert_eq!(
            mock.execute_verified(target, &click, Some(wrong_window)),
            refused
        );
        assert!(mock.executed.is_empty());

        let yes = confirmed(target, click.clone());
        assert_eq!(mock.execute_verified(target, &click, Some(yes)), Ok(()));
        assert_eq!(mock.executed, vec![(target, click)]);
    }

    fn confirmed(target: TargetWindow, action: InputAction) -> HumanConfirmation {
        let element = crate::ConfirmedElement {
            runtime_id: vec![1, 42],
            process_id: target.process_id,
            name: "Open".into(),
            control_type: "Button".into(),
            rect: PhysRect::new(0, 0, 100, 100).unwrap(),
            enabled: true,
            offscreen: false,
            blocked: false,
        };
        crate::ask_confirmation(target, &action, element, || {})
            .unwrap()
            .answer(true)
            .unwrap()
    }

    #[test]
    fn other_windows_and_empty_queues_are_errors() {
        let mut mock = backend();
        let other = TargetWindow {
            handle: 8,
            process_id: 42,
        };
        assert!(matches!(
            mock.snapshot(other),
            Err(PlatformError::WindowNotFound)
        ));
        let captured = mock.capture_target(mock.snapshot.target);
        assert!(matches!(captured, Err(PlatformError::CaptureFailed)));
        assert!(mock.poll_event().is_none());
    }
}
