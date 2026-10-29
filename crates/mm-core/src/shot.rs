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
    paint_black_channels(pixels, width, height, rects, 4)
}

fn paint_black_channels(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    rects: &[ShotRectPx],
    channels: usize,
) -> Result<usize, ShotError> {
    validate(pixels, width, height, channels)?;
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
            let start = (y * width as usize + left as usize) * channels;
            let end = (y * width as usize + right as usize) * channels;
            pixels[start..end].fill(0);
            if channels == 4 {
                for alpha in pixels[start + 3..end].iter_mut().step_by(4) {
                    *alpha = 255;
                }
            }
        }
        painted += 1;
    }
    Ok(painted)
}

/// Area-average to packed RGB8, ignoring alpha. Dimensions round down and stay
/// at least one pixel. Fixed-point means are within one level of the exact
/// rounded-down channel mean. Zero keeps the size.
pub fn downscale_to_rgb(src: Bgra<'_>, max_edge: u32) -> Result<(Vec<u8>, u32, u32), ShotError> {
    // Public fields permit unchecked struct literals.
    validate(src.pixels, src.width, src.height, 4)?;
    downscale(src.pixels, src.width, src.height, 4, [2, 1, 0], max_edge)
}

// Redaction needs an owned working buffer, but never an owned BGRA copy: alpha
// is ignored by the model. Paint RGB before filtering so no hidden pixel leaks.
pub(crate) fn redact_to_rgb(
    src: Bgra<'_>,
    rects: &[ShotRectPx],
    max_edge: u32,
) -> Result<(Vec<u8>, u32, u32, usize), ShotError> {
    let (mut rgb, width, height) = downscale_to_rgb(src, 0)?;
    let painted = paint_black_channels(&mut rgb, width, height, rects, 3)?;
    if max_edge == 0 || width.max(height) <= max_edge {
        return Ok((rgb, width, height, painted));
    }
    let (rgb, width, height) = downscale(&rgb, width, height, 3, [0, 1, 2], max_edge)?;
    Ok((rgb, width, height, painted))
}

fn downscale(
    pixels: &[u8],
    src_width: u32,
    src_height: u32,
    channels: usize,
    offsets: [usize; 3],
    max_edge: u32,
) -> Result<(Vec<u8>, u32, u32), ShotError> {
    let long = src_width.max(src_height);
    let (width, height) = if max_edge == 0 || long <= max_edge {
        (src_width, src_height)
    } else {
        let scale = |n| ((u64::from(n) * u64::from(max_edge) / u64::from(long)) as u32).max(1);
        (scale(src_width), scale(src_height))
    };
    let mut rgb = vec![0; buffer_len(width, height, 3)?];
    if (width, height) == (src_width, src_height) {
        for (src, dst) in pixels
            .chunks_exact(channels)
            .zip(rgb.as_chunks_mut::<3>().0.iter_mut())
        {
            dst[0] = src[offsets[0]];
            dst[1] = src[offsets[1]];
            dst[2] = src[offsets[2]];
        }
        return Ok((rgb, width, height));
    }
    // Exact integer overlap coordinates, with horizontal means in Q16. Only
    // that division loses precision: the final channel differs by at most one
    // from the exact area mean. Even u32::MAX dimensions fit these u64 sums:
    // horizontal <= 255 * sw, vertical <= (255 << 16) * sh.
    let sw = u64::from(src_width);
    let sh = u64::from(src_height);
    let dw = u64::from(width);
    let dh = u64::from(height);
    let spans: Vec<_> = (0..dw).map(|x| (x * sw, (x + 1) * sw)).collect();
    let mut row = vec![0u64; width as usize * 3];
    let mut sums = vec![0u64; row.len()];
    let mut cached_row = None;
    for y in 0..dh {
        let top = y * sh;
        let bottom = (y + 1) * sh;
        sums.fill(0);
        for sy in top / dh..bottom.div_ceil(dh) {
            if cached_row != Some(sy) {
                for (&(left, right), dst) in spans.iter().zip(row.as_chunks_mut::<3>().0.iter_mut())
                {
                    let mut horizontal = [0u64; 3];
                    for sx in left / dw..right.div_ceil(dw) {
                        let wx = right.min((sx + 1) * dw) - left.max(sx * dw);
                        let i = (sy as usize * src_width as usize + sx as usize) * channels;
                        for channel in 0..3 {
                            horizontal[channel] += u64::from(pixels[i + offsets[channel]]) * wx;
                        }
                    }
                    for channel in 0..3 {
                        dst[channel] = (horizontal[channel] << 16) / sw;
                    }
                }
                cached_row = Some(sy);
            }
            let wy = bottom.min((sy + 1) * dh) - top.max(sy * dh);
            for (sum, value) in sums.iter_mut().zip(&row) {
                *sum += value * wy;
            }
        }
        let start = y as usize * row.len();
        for (dst, sum) in rgb[start..start + row.len()].iter_mut().zip(&sums) {
            *dst = ((sum / sh) >> 16) as u8;
        }
    }
    Ok((rgb, width, height))
}

/// Encode packed RGB8 to an in-memory PNG with the encoder's default compression: the fast
/// mode saved ~65 ms but made the upload up to 55x larger (WP113 measurement), a bad trade.
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
