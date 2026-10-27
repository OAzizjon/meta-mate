//! The redaction step (invariant 3, convention 4, D37): the only producer of
//! [`RedactedImage`]. It refuses when redaction cannot be guaranteed (fail closed),
//! paints every field that [`must_hide`] marks, then downscales and encodes the frame.

use std::fmt;

use crate::api::request::{RedactedImage, must_hide};
use crate::geom::{FrameTransform, PhysRect};
use crate::shot::{self, Bgra, ShotError, ShotRectPx};

/// One element with bounds, as the UIA snapshot reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub rect: PhysRect,
    pub is_password: bool,
    pub password_like: bool,
    pub was_password_this_session: bool,
}

/// What the capture and the snapshot say about one request.
pub struct RedactionInput<'a> {
    /// The captured region (the target window plus its popups), physical pixels.
    pub capture: Bgra<'a>,
    /// Where `capture` lies on the virtual screen.
    pub region: PhysRect,
    pub fields: &'a [Field],
    /// The UIA walk hit its limit or its completeness is unknown.
    pub snapshot_truncated: bool,
    /// D37: an edit field on a login or payment page that could not be classified.
    pub unclassified_sensitive_field: bool,
    /// Long edge of the image sent to the model (0 keeps the capture size).
    pub max_edge: u32,
}

/// Why nothing may be sent: the caller asks the person first (copy.md "Login page",
/// "Private window") instead of capturing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactionError {
    SnapshotTruncated,
    UnclassifiedSensitiveField,
    /// The capture's size is not the region's: the field rects cannot be placed.
    CaptureMismatch,
    Shot(ShotError),
}

impl fmt::Display for RedactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotTruncated => f.write_str("the UIA snapshot is incomplete"),
            Self::UnclassifiedSensitiveField => f.write_str("a field could not be classified"),
            Self::CaptureMismatch => f.write_str("the capture does not match its region"),
            Self::Shot(error) => write!(f, "screenshot: {error}"),
        }
    }
}

impl std::error::Error for RedactionError {}

/// A redacted screenshot ready for a request, with the transform of this frame
/// (invariant 2: the model answers in pixels of exactly this image).
#[derive(Debug)]
pub struct Redacted {
    pub image: RedactedImage,
    pub frame: FrameTransform,
    pub painted: usize,
}

pub fn redact(input: &RedactionInput<'_>) -> Result<Redacted, RedactionError> {
    if input.snapshot_truncated {
        return Err(RedactionError::SnapshotTruncated);
    }
    if input.unclassified_sensitive_field {
        return Err(RedactionError::UnclassifiedSensitiveField);
    }
    let region = input.region;
    let (width, height) = (input.capture.width, input.capture.height);
    if i64::from(region.right) - i64::from(region.left) != i64::from(width)
        || i64::from(region.bottom) - i64::from(region.top) != i64::from(height)
    {
        return Err(RedactionError::CaptureMismatch);
    }

    let rects: Vec<ShotRectPx> = input
        .fields
        .iter()
        .filter(|f| must_hide(f.is_password, f.password_like, f.was_password_this_session))
        .map(|f| ShotRectPx {
            left: i64::from(f.rect.left) - i64::from(region.left),
            top: i64::from(f.rect.top) - i64::from(region.top),
            right: i64::from(f.rect.right) - i64::from(region.left),
            bottom: i64::from(f.rect.bottom) - i64::from(region.top),
        })
        .collect();
    let mut pixels = input.capture.pixels.to_vec();
    let painted =
        shot::paint_black(&mut pixels, width, height, &rects).map_err(RedactionError::Shot)?;

    let painted_view = Bgra::new(width, height, &pixels).map_err(RedactionError::Shot)?;
    let (rgb, sent_width, sent_height) =
        shot::downscale_to_rgb(painted_view, input.max_edge).map_err(RedactionError::Shot)?;
    let png = shot::encode_png_rgb(&rgb, sent_width, sent_height).map_err(RedactionError::Shot)?;
    let frame = FrameTransform::new(region, sent_width, sent_height)
        .map_err(|_| RedactionError::CaptureMismatch)?;
    Ok(Redacted {
        image: RedactedImage::from_redacted_data_url(shot::png_data_url(&png)),
        frame,
        painted,
    })
}
