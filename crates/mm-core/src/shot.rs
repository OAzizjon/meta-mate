//! Pure pixel preparation for model screenshots. Redact before downscaling.
//!
//! These helpers do not certify redaction: the caller must establish that every
//! sensitive region is covered before constructing an API `RedactedImage`.

use std::fmt;

use base64::{Engine, engine::general_purpose::STANDARD};

pub const REDACTION_PAD_PX: i64 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotError {
    InvalidImageSize,
    SizeOverflow,
    InvalidBufferLength { expected: usize, actual: usize },
    PngEncoding,
}

impl fmt::Display for ShotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidImageSize => f.write_str("image dimensions must be positive"),
            Self::SizeOverflow => f.write_str("image buffer size overflows"),
            Self::InvalidBufferLength { expected, actual } => {
                write!(f, "image buffer length is {actual}, expected {expected}")
            }
            Self::PngEncoding => f.write_str("PNG encoding failed"),
        }
    }
}

impl std::error::Error for ShotError {}

/// Packed BGRA8 capture pixels; alpha is ignored when producing model RGB.
#[derive(Clone, Copy)]
pub struct Bgra<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a [u8],
}

impl<'a> Bgra<'a> {
    pub fn new(width: u32, height: u32, pixels: &'a [u8]) -> Result<Self, ShotError> {
        validate(pixels, width, height, 4)?;
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
}

/// Metadata only (convention 3); never log capture contents.
impl fmt::Debug for Bgra<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bgra")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.pixels.len())
            .finish()
    }
}

fn buffer_len(width: u32, height: u32, channels: usize) -> Result<usize, ShotError> {
    if width == 0 || height == 0 {
        return Err(ShotError::InvalidImageSize);
    }
    usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|n| n.checked_mul(channels))
        .filter(|n| *n <= isize::MAX as usize)
        .ok_or(ShotError::SizeOverflow)
}

fn validate(pixels: &[u8], width: u32, height: u32, channels: usize) -> Result<(), ShotError> {
    let expected = buffer_len(width, height, channels)?;
    if pixels.len() != expected {
        return Err(ShotError::InvalidBufferLength {
            expected,
            actual: pixels.len(),
        });
    }
    Ok(())
}

/// Half-open rectangle in capture pixels, before downscaling (not screen pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShotRectPx {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

/// Pad then clip valid rectangles and paint opaque black. Empty/inverted rects
/// and padded rectangles fully outside the image are skipped, without counting.
/// Overlapping rectangles are each counted when they have a nonempty clip.
pub fn paint_black(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    rects: &[ShotRectPx],
) -> Result<usize, ShotError> {
    validate(pixels, width, height, 4)?;
    let mut painted = 0;
    for rect in rects {
        if rect.left >= rect.right || rect.top >= rect.bottom {
            continue;
        }
        let left = rect.left.saturating_sub(REDACTION_PAD_PX).max(0);
        let top = rect.top.saturating_sub(REDACTION_PAD_PX).max(0);
        let right = rect
            .right
            .saturating_add(REDACTION_PAD_PX)
            .min(i64::from(width));
        let bottom = rect
            .bottom
            .saturating_add(REDACTION_PAD_PX)
            .min(i64::from(height));
        if left >= right || top >= bottom {
            continue;
        }
        for y in top as usize..bottom as usize {
            let start = (y * width as usize + left as usize) * 4;
            let end = (y * width as usize + right as usize) * 4;
            for pixel in pixels[start..end].as_chunks_mut::<4>().0 {
                *pixel = [0, 0, 0, 255];
            }
        }
        painted += 1;
    }
    Ok(painted)
}

/// Area-average to packed RGB8, ignoring alpha. Dimensions and channel means
/// round down; each dimension stays at least one pixel. Zero keeps the size.
pub fn downscale_to_rgb(src: Bgra<'_>, max_edge: u32) -> Result<(Vec<u8>, u32, u32), ShotError> {
    // Public fields permit unchecked struct literals.
    validate(src.pixels, src.width, src.height, 4)?;
    let long = src.width.max(src.height);
    let (width, height) = if max_edge == 0 || long <= max_edge {
        (src.width, src.height)
    } else {
        let scale = |n| ((u64::from(n) * u64::from(max_edge) / u64::from(long)) as u32).max(1);
        (scale(src.width), scale(src.height))
    };
    let mut rgb = vec![0; buffer_len(width, height, 3)?];
    // Coordinates are integers in units of 1/output_dimension source pixels.
    // Each destination footprint has area src.width * src.height in these units.
    // u64 holds coordinate products; u128 also holds weighted channel sums.
    let sw = u64::from(src.width);
    let sh = u64::from(src.height);
    let dw = u64::from(width);
    let dh = u64::from(height);
    let area = u128::from(sw) * u128::from(sh);
    for y in 0..dh {
        let top = y * sh;
        let bottom = (y + 1) * sh;
        for x in 0..dw {
            let left = x * sw;
            let right = (x + 1) * sw;
            let mut sums = [0u128; 3];
            for sy in top / dh..bottom.div_ceil(dh) {
                let wy = bottom.min((sy + 1) * dh) - top.max(sy * dh);
                for sx in left / dw..right.div_ceil(dw) {
                    let wx = right.min((sx + 1) * dw) - left.max(sx * dw);
                    let weight = u128::from(wx) * u128::from(wy);
                    let i = (sy as usize * src.width as usize + sx as usize) * 4;
                    for (channel, offset) in [2, 1, 0].into_iter().enumerate() {
                        sums[channel] += u128::from(src.pixels[i + offset]) * weight;
                    }
                }
            }
            let out = (y as usize * width as usize + x as usize) * 3;
            for channel in 0..3 {
                rgb[out + channel] = (sums[channel] / area) as u8;
            }
        }
    }
    Ok((rgb, width, height))
}

/// Encode packed RGB8 to an in-memory PNG, using the encoder's default compression.
pub fn encode_png_rgb(rgb: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ShotError> {
    validate(rgb, width, height, 3)?;
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|_| ShotError::PngEncoding)?;
        writer
            .write_image_data(rgb)
            .map_err(|_| ShotError::PngEncoding)?;
        writer.finish().map_err(|_| ShotError::PngEncoding)?;
    }
    Ok(bytes)
}

pub fn png_data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", STANDARD.encode(png))
}
