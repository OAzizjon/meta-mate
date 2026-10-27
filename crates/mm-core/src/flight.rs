//! Mate's window flight path between two physical screen rectangles.

use crate::geom::PhysRect;

pub const MIN_DURATION_MS: u32 = 150;
pub const MAX_DURATION_MS: u32 = 900;
pub const MAX_ARC_PX: i32 = 80;

/// Cubic ease-in-out on `0.0..=1.0`.
fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Computes Mate's rectangle for each animation frame from `from` to `to`.
///
/// Every returned rectangle has the same width and height as `to`. The path uses
/// cubic ease-in-out and lifts the midpoint upward by `min(80, distance / 4)` physical
/// pixels, never rising above `from.top.min(to.top) - 80`. The first frame is after
/// `from` and the last frame equals `to` exactly; `from == to` returns `vec![to]`.
pub fn flight_path(from: PhysRect, to: PhysRect, duration_ms: u32, frame_ms: u32) -> Vec<PhysRect> {
    if from == to {
        return vec![to];
    }

    let duration = duration_ms.clamp(MIN_DURATION_MS, MAX_DURATION_MS);
    let frames = duration.checked_div(frame_ms).unwrap_or(0).max(1) as usize;
    if frames == 1 {
        return vec![to];
    }

    let dx = f64::from(to.left) - f64::from(from.left);
    let dy = f64::from(to.top) - f64::from(from.top);
    let distance = dx.hypot(dy);
    let lift = (distance / 4.0).min(f64::from(MAX_ARC_PX));
    let min_top = from.top.min(to.top).saturating_sub(MAX_ARC_PX);
    let width = i64::from(to.right) - i64::from(to.left);
    let height = i64::from(to.bottom) - i64::from(to.top);

    let mut path = Vec::with_capacity(frames);
    for i in 1..frames {
        let t = i as f64 / frames as f64;
        let u = ease_in_out_cubic(t);
        let arc = 4.0 * lift * u * (1.0 - u);

        let mut left = (f64::from(from.left) + dx * u).round() as i32;
        let mut top = (f64::from(from.top) + dy * u - arc)
            .round()
            .max(f64::from(min_top)) as i32;

        if left == from.left && to.left != from.left {
            left = if to.left > from.left {
                left.saturating_add(1)
            } else {
                left.saturating_sub(1)
            };
        } else if left == from.left && top == from.top && to.top != from.top {
            top = if to.top > from.top {
                top.saturating_add(1)
            } else {
                top.saturating_sub(1).max(min_top)
            };
        }

        let right =
            (i64::from(left) + width).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        let bottom =
            (i64::from(top) + height).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        path.push(PhysRect {
            left,
            top,
            right,
            bottom,
        });
    }
    path.push(to);
    path
}
