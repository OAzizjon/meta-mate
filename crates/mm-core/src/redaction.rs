//! The redaction step (invariant 3, convention 4, D37): the only producer of
//! [`RedactedImage`]. It refuses when redaction cannot be guaranteed (fail closed),
//! paints every field that [`must_hide`] marks, then downscales and encodes the frame.

use std::fmt;

use crate::api::request::must_hide;
use crate::geom::{FrameTransform, PhysPoint, PhysRect, uncovered};
use crate::shot::{self, Bgra, ShotError, ShotRectPx};

/// Parts of the capture outside the target and its popups, plus other apps above it.
/// Empty/inverted rectangles are ignored; occluders are clipped to the capture.
/// Rectangles may overlap, since painting the same pixel twice is harmless.
pub fn painted_areas(
    region: PhysRect,
    window: PhysRect,
    popups: &[PhysRect],
    occluders: &[PhysRect],
) -> Vec<PhysRect> {
    let mut covers = Vec::with_capacity(popups.len() + 1);
    covers.push(window);
    covers.extend_from_slice(popups);
    let mut painted = uncovered(region, &covers);
    for occluder in occluders {
        let clipped = PhysRect {
            left: region.left.max(occluder.left),
            top: region.top.max(occluder.top),
            right: region.right.min(occluder.right),
            bottom: region.bottom.min(occluder.bottom),
        };
        if clipped.left < clipped.right && clipped.top < clipped.bottom {
            painted.push(clipped);
        }
    }
    painted
}

/// Whether any pixel of a valid half-open rectangle lies in a painted area.
pub fn overlaps_any(rect: PhysRect, painted: &[PhysRect]) -> bool {
    painted.iter().any(|area| {
        rect.left.max(area.left) < rect.right.min(area.right)
            && rect.top.max(area.top) < rect.bottom.min(area.bottom)
    })
}

/// Includes left/top and excludes right/bottom, just like `PhysRect::contains`.
pub fn point_in_any(point: PhysPoint, painted: &[PhysRect]) -> bool {
    painted.iter().any(|area| area.contains(point))
}

/// A screenshot whose password fields were painted over, as a data URL (convention 4).
/// Only [`redact`] can make one (its constructor is private to this module), so the
/// request builders, which take nothing else, can never send a raw capture.
pub struct RedactedImage {
    data_url: String,
}

impl RedactedImage {
    fn new(data_url: String) -> Self {
        Self { data_url }
    }

    /// For the request builders.
    pub(crate) fn data_url(&self) -> &str {
        &self.data_url
    }

    #[cfg(test)]
    pub(crate) fn for_tests(data_url: String) -> Self {
        Self::new(data_url)
    }
}

/// Metadata only (convention 3).
impl fmt::Debug for RedactedImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[RedactedImage {} bytes]", self.data_url.len())
    }
}

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
    /// The region the UIA snapshot (and so every field rect) was taken against. A
    /// window that moved between the snapshot and the capture would get its fields
    /// painted in the wrong place: it must equal `region`.
    pub snapshot_region: PhysRect,
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
    /// The window moved or resized between the UIA snapshot and the capture.
    StaleSnapshot,
    /// A field that must be hidden has empty or inverted bounds: where it is is unknown.
    InvalidField,
    Shot(ShotError),
}

impl fmt::Display for RedactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotTruncated => f.write_str("the UIA snapshot is incomplete"),
            Self::UnclassifiedSensitiveField => f.write_str("a field could not be classified"),
            Self::CaptureMismatch => f.write_str("the capture does not match its region"),
            Self::StaleSnapshot => f.write_str("the window moved after the UIA snapshot"),
            Self::InvalidField => f.write_str("a hidden field has invalid bounds"),
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
    if input.snapshot_region != region {
        return Err(RedactionError::StaleSnapshot);
    }
    let (width, height) = (input.capture.width, input.capture.height);
    if i64::from(region.right) - i64::from(region.left) != i64::from(width)
        || i64::from(region.bottom) - i64::from(region.top) != i64::from(height)
    {
        return Err(RedactionError::CaptureMismatch);
    }

    let hidden: Vec<&Field> = input
        .fields
        .iter()
        .filter(|f| must_hide(f.is_password, f.password_like, f.was_password_this_session))
        .collect();
    // Fail closed: a hidden field we cannot place is not silently skipped.
    if hidden
        .iter()
        .any(|f| f.rect.left >= f.rect.right || f.rect.top >= f.rect.bottom)
    {
        return Err(RedactionError::InvalidField);
    }
    let rects: Vec<ShotRectPx> = hidden
        .into_iter()
        .map(|f| ShotRectPx {
            left: i64::from(f.rect.left) - i64::from(region.left),
            top: i64::from(f.rect.top) - i64::from(region.top),
            right: i64::from(f.rect.right) - i64::from(region.left),
            bottom: i64::from(f.rect.bottom) - i64::from(region.top),
        })
        .collect();
    let (rgb, sent_width, sent_height, painted) =
        shot::redact_to_rgb(input.capture, &rects, input.max_edge).map_err(RedactionError::Shot)?;
    let png = shot::encode_png_rgb(&rgb, sent_width, sent_height).map_err(RedactionError::Shot)?;
    let frame = FrameTransform::new(region, sent_width, sent_height)
        .map_err(|_| RedactionError::CaptureMismatch)?;
    Ok(Redacted {
        image: RedactedImage::new(shot::png_data_url(&png)),
        frame,
        painted,
    })
}
