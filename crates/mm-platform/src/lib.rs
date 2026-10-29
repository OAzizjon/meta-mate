//! OS services behind traits (`InputHooks`, `UiTree`, `ScreenCapture`, `Overlay`,
//! `InputInjector`), with `MockBackend` and `ReplayBackend` so pointing logic is tested
//! against recorded screens in `fixtures/`, not a live desktop (docs/project-structure.md).

mod captured_image;
mod error;
mod input_hooks;
mod input_injector;
mod mock;
mod overlay;
mod recent_targets;
mod replay;
mod screen_capture;
mod target;
mod ui_snapshot;
mod ui_tree;

pub use captured_image::CapturedImage;
pub use error::PlatformError;
pub use input_hooks::{InputEvent, InputHooks, MouseButton};
pub use input_injector::{
    ConfirmedElement, HumanConfirmation, InputAction, InputInjector, PendingConfirmation,
    ask_confirmation, check_confirmation, recheck_element,
};
pub use mock::MockBackend;
pub use overlay::{Overlay, RobotPose, RobotState};
pub use recent_targets::RecentTargets;
pub use replay::{REPLAY_TARGET, ReplayBackend};
pub use screen_capture::ScreenCapture;
pub use target::TargetWindow;
pub use ui_snapshot::{UiElement, UiSnapshot, is_interactive_type, same_hidden};
pub use ui_tree::UiTree;
