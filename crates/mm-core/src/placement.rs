//! Place Mate beside a target in physical virtual-screen pixels.

use crate::geom::{PhysPoint, PhysRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Right,
    Left,
    Below,
    Above,
    Corner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub mate: PhysRect,
    pub beam_from: PhysPoint,
    pub side: Side,
}

/// Prefer right, left, below, then above, with clearance around the target.
/// If all sides fail, use the farthest work-area corner (which may overlap).
/// Corner distance ties prefer left/top; beam-corner ties do likewise.
/// Nonpositive Mate dimensions cannot fit; negative gaps mean zero clearance.
pub fn place_mate(
    target: PhysRect,
    work_area: PhysRect,
    mate_width: i32,
    mate_height: i32,
    gap: i32,
) -> Option<Placement> {
    // Wide intermediates also cover monitors spanning the entire i32 range.
    let left = i64::from(work_area.left);
    let top = i64::from(work_area.top);
    let right = i64::from(work_area.right);
    let bottom = i64::from(work_area.bottom);
    let width = i64::from(mate_width);
    let height = i64::from(mate_height);
    if width <= 0 || height <= 0 || width > right - left || height > bottom - top {
        return None;
    }

    let gap = i64::from(gap.max(0));
    let inflated_left = i64::from(target.left) - gap;
    let inflated_top = i64::from(target.top) - gap;
    let inflated_right = i64::from(target.right) + gap;
    let inflated_bottom = i64::from(target.bottom) + gap;
    let center = target.center();
    let centered_x = i64::from(center.x) - width / 2;
    let centered_y = i64::from(center.y) - height / 2;

    for (side, x, y) in [
        (Side::Right, inflated_right, centered_y),
        (Side::Left, inflated_left - width, centered_y),
        (Side::Below, centered_x, inflated_bottom),
        (Side::Above, centered_x, inflated_top - height),
    ] {
        let x = x.clamp(left, right - width);
        let y = y.clamp(top, bottom - height);
        // Clamping a preferred position must not move Mate into the clearance.
        if x < inflated_right
            && x + width > inflated_left
            && y < inflated_bottom
            && y + height > inflated_top
        {
            continue;
        }
        let mate = rect_at(x, y, width, height);
        let middle = mate.center();
        let beam_from = match side {
            Side::Right => PhysPoint {
                x: mate.left,
                y: middle.y,
            },
            Side::Left => PhysPoint {
                x: mate.right,
                y: middle.y,
            },
            Side::Below => PhysPoint {
                x: middle.x,
                y: mate.top,
            },
            Side::Above => PhysPoint {
                x: middle.x,
                y: mate.bottom,
            },
            Side::Corner => continue, // never in this loop
        };
        return Some(Placement {
            mate,
            beam_from,
            side,
        });
    }

    // Squared distance to an axis-aligned corner is maximized independently
    // along each axis. Doubled centers preserve half-pixel ties exactly.
    let target_x = i64::from(target.left) + i64::from(target.right);
    let target_y = i64::from(target.top) + i64::from(target.bottom);
    let x = if target_x < left + right {
        right - width
    } else {
        left
    };
    let y = if target_y < top + bottom {
        bottom - height
    } else {
        top
    };
    let mate = rect_at(x, y, width, height);
    let beam_from = PhysPoint {
        x: if target_x > x + x + width {
            mate.right
        } else {
            mate.left
        },
        y: if target_y > y + y + height {
            mate.bottom
        } else {
            mate.top
        },
    };
    Some(Placement {
        mate,
        beam_from,
        side: Side::Corner,
    })
}

fn rect_at(x: i64, y: i64, width: i64, height: i64) -> PhysRect {
    // Callers clamp into an i32 work area before converting back.
    PhysRect {
        left: x as i32,
        top: y as i32,
        right: (x + width) as i32,
        bottom: (y + height) as i32,
    }
}
