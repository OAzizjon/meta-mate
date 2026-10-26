//! A raw screenshot of the target region.

use std::fmt;

use crate::error::PlatformError;

/// Pixels of one capture: 8-bit BGRA (the Windows capture format), rows top to bottom, no
/// padding. Raw and unredacted, so it never leaves the PC as is (convention 4).
pub struct CapturedImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl CapturedImage {
    /// Bytes per pixel (BGRA).
    pub const BYTES_PER_PIXEL: usize = 4;

    /// Wraps exactly `width * height * 4` bytes; an empty or mismatched buffer is
    /// `CaptureFailed`.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, PlatformError> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|count| count.checked_mul(Self::BYTES_PER_PIXEL));
        if width == 0 || height == 0 || expected != Some(pixels.len()) {
            return Err(PlatformError::CaptureFailed);
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Mutable pixels for in-place redaction; the buffer size cannot change.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }
}

/// Metadata only, never pixels (convention 3).
impl fmt::Debug for CapturedImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[CapturedImage {}x{}]", self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_prints_metadata_only() {
        let pixels = vec![0xAB; 8];
        let image = CapturedImage::new(2, 1, pixels).expect("valid buffer");
        assert_eq!(format!("{image:?}"), "[CapturedImage 2x1]");
        assert_eq!(format!("{image:#?}"), "[CapturedImage 2x1]");
    }

    #[test]
    fn buffer_must_match_the_size() {
        let failed = Some(PlatformError::CaptureFailed);
        assert_eq!(CapturedImage::new(2, 2, vec![0; 15]).err(), failed);
        assert_eq!(CapturedImage::new(0, 2, Vec::new()).err(), failed);
        let image = CapturedImage::new(2, 2, vec![0; 16]).expect("valid");
        assert_eq!((image.width(), image.height()), (2, 2));
        assert_eq!(image.pixels().len(), 16);
    }
}
