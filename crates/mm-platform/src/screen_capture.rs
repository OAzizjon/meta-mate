//! Capturing the target region together with its frame transform.

use mm_core::geom::FrameTransform;

use crate::captured_image::CapturedImage;
use crate::error::PlatformError;
use crate::target::TargetWindow;

/// Screenshots of the user's window.
pub trait ScreenCapture {
    /// Captures `target` plus its owned popups, only during a request (invariant 3), with the
    /// one transform of this frame (invariant 2). Our own windows must not be in the image.
    fn capture_target(
        &mut self,
        target: TargetWindow,
    ) -> Result<(CapturedImage, FrameTransform), PlatformError>;
}
