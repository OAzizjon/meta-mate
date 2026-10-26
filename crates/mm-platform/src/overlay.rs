//! Mate's window and the highlight, placed in physical pixels.

use mm_core::geom::{PhysPoint, PhysRect};

use crate::error::PlatformError;

/// What Mate is doing (docs/robot.md, "States"). His face (mood) comes from MateScript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RobotState {
    Idle,
    Listening,
    Thinking,
    Speaking,
    Flying,
    Pointing,
    WaitingForClick,
    Success,
    Oops,
    CantFind,
    Sleeping,
}

/// Where Mate is and what he is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RobotPose {
    /// The centre of Mate's body in physical virtual-screen pixels; never on the target.
    pub at: PhysPoint,
    pub state: RobotState,
}

/// Our on-screen windows. No call ever takes focus from the user's app (invariant 1).
pub trait Overlay {
    fn set_robot_pose(&mut self, pose: RobotPose) -> Result<(), PlatformError>;

    /// Outlines `rect` (physical pixels), or removes the outline with `None`.
    fn set_highlight(&mut self, rect: Option<PhysRect>) -> Result<(), PlatformError>;

    /// Hides Mate and the highlight for a capture frame (`true`) and shows them again
    /// (`false`). Never by excluding our windows from capture: Zoom and OBS must see Mate.
    fn set_hidden_for_capture(&mut self, hidden: bool) -> Result<(), PlatformError>;
}
