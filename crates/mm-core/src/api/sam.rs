//! SAM 3.1 point-prompt request and bounding-box parser (docs/prompts.md §4).
//!
//! Apps with a flat or empty UI Automation tree (Qt, canvas pages, games) leave the model
//! with no element id to point at; when it answers `<point X,Y>` and no pointable UIA
//! element lies under that pixel, SAM 3.1 outlines the control under `(x, y)` in the same
//! redacted screenshot.

use serde_json::{Value, json};

pub use crate::redaction::RedactedImage;

const SAM_MODEL: &str = "sam-3.1";
const BOX_PREFIX: &str = "<|box;";
const TAG_END: &str = "|>";
const MIN_BOX_EDGE: u32 = 8;
const MAX_AREA_PERCENT: u128 = 40;

/// Half-open bounding box in pixels of the image sent to the model (`right = x2 + 1`,
/// `bottom = y2 + 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SamBox {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl SamBox {
    /// Width in screenshot pixels.
    pub fn width(self) -> u32 {
        self.right.saturating_sub(self.left)
    }

    /// Height in screenshot pixels.
    pub fn height(self) -> u32 {
        self.bottom.saturating_sub(self.top)
    }

    /// Area in screenshot pixels.
    pub fn area(self) -> u64 {
        u64::from(self.width()) * u64::from(self.height())
    }

    /// Includes `left`/`top` and excludes `right`/`bottom` (`x2`/`y2` inclusive).
    pub fn contains(self, x: u32, y: u32) -> bool {
        self.left <= x && x < self.right && self.top <= y && y < self.bottom
    }
}

/// Streamed `/v1/responses` body for a SAM 3.1 point prompt on a [`RedactedImage`]
/// (invariant 3). Caller ensures `x < w` and `y < h`; coordinates are clamped into `sent`
/// (`(w, h)`) regardless.
pub fn sam_point_request(x: u32, y: u32, sent: (u32, u32), image: &RedactedImage) -> Value {
    let (w, h) = sent;
    let x = x.min(w.saturating_sub(1));
    let y = y.min(h.saturating_sub(1));
    let prompt = format!("<0f>1<|point;x={x};y={y};w={w};h={h}|>");
    json!({
        "model": SAM_MODEL,
        "stream": true,
        "input": [{
            "type": "message",
            "role": "user",
            "content": [
                {"type": "input_text", "text": prompt},
                {"type": "input_image", "image_url": image.data_url()}
            ]
        }]
    })
}

/// Extracts every valid `<|box;x1=..;y1=..;x2=..;y2=..;w=..;h=..|>` from the accumulated
/// streamed text for an image of size `sent` (`(w, h)`).
///
/// Boxes whose `w`/`h` differ from `sent`, whose numbers do not parse, or with
/// `x1 > x2`, `y1 > y2`, `x2 >= w`, or `y2 >= h` are skipped. `<|mask;...|>` payloads are
/// ignored without allocating their data.
pub fn parse_sam_boxes(text: &str, sent: (u32, u32)) -> Vec<SamBox> {
    let mut boxes = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(BOX_PREFIX) {
        rest = &rest[start + BOX_PREFIX.len()..];
        let Some(end) = rest.find(TAG_END) else {
            break;
        };
        if rest.find(BOX_PREFIX).is_some_and(|next| next < end) {
            continue;
        }
        if let Some(sam_box) = parse_box_fields(&rest[..end], sent) {
            boxes.push(sam_box);
        }
        rest = &rest[end + TAG_END.len()..];
    }
    boxes
}

/// Picks the smallest box in `boxes` that contains `(x, y)`, is at least 8×8 px, and
/// covers at most 40% of the `sent` image (`docs/prompts.md` §4).
pub fn pick_sam_box(boxes: &[SamBox], x: u32, y: u32, sent: (u32, u32)) -> Option<SamBox> {
    let (w, h) = sent;
    if w == 0 || h == 0 || x >= w || y >= h {
        return None;
    }
    let image_area = u128::from(w) * u128::from(h);
    boxes
        .iter()
        .copied()
        .filter(|b| {
            b.left < b.right
                && b.top < b.bottom
                && b.right <= w
                && b.bottom <= h
                && b.contains(x, y)
                && b.width() >= MIN_BOX_EDGE
                && b.height() >= MIN_BOX_EDGE
                && u128::from(b.area()) * 100 <= image_area * MAX_AREA_PERCENT
        })
        .min_by_key(|b| b.area())
}

fn parse_box_fields(fields: &str, (sent_w, sent_h): (u32, u32)) -> Option<SamBox> {
    let mut parts = fields.split(';');
    let x1 = parse_field(parts.next()?, "x1=")?;
    let y1 = parse_field(parts.next()?, "y1=")?;
    let x2 = parse_field(parts.next()?, "x2=")?;
    let y2 = parse_field(parts.next()?, "y2=")?;
    let w = parse_field(parts.next()?, "w=")?;
    let h = parse_field(parts.next()?, "h=")?;
    if parts.next().is_some() {
        return None;
    }
    if w != sent_w || h != sent_h || x1 > x2 || y1 > y2 || x2 >= w || y2 >= h {
        return None;
    }
    Some(SamBox {
        left: x1,
        top: y1,
        right: x2 + 1,
        bottom: y2 + 1,
    })
}

fn parse_field(part: &str, prefix: &str) -> Option<u32> {
    let digits = part.strip_prefix(prefix)?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}
