//! Reading the target's UI Automation tree.

use mm_core::geom::PhysPoint;

use crate::error::PlatformError;
use crate::target::TargetWindow;
use crate::ui_snapshot::{UiElement, UiSnapshot};

/// The accessibility tree of the user's window.
pub trait UiTree {
    /// A bounded walk (about 400 elements) of `target` and its owned popups, our own windows
    /// excluded.
    fn snapshot(&self, target: TargetWindow) -> Result<UiSnapshot, PlatformError>;

    /// The element under a physical virtual-screen point, our own windows excluded.
    /// `Ok(None)` means nothing usable is there: say "I can't find it" (invariant 5).
    fn element_from_point(&self, point: PhysPoint) -> Result<Option<UiElement>, PlatformError>;
}
