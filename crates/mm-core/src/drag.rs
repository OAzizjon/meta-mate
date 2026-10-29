//! Physical-pixel dragging and restoration, with no window or input dependencies.

use crate::geom::{PhysPoint, PhysRect};

/// Keeps the original grab offset, even after hitting an edge of the work area.
#[derive(Debug, Clone, Copy)]
pub struct Drag {
    start: PhysRect,
    cursor: PhysPoint,
}

impl Drag {
    pub fn new(start: PhysRect, cursor: PhysPoint) -> Self {
        Self { start, cursor }
    }

    pub fn at(self, cursor: PhysPoint, work: PhysRect) -> Option<PhysRect> {
        clamped(
            i64::from(self.start.left) + i64::from(cursor.x) - i64::from(self.cursor.x),
            i64::from(self.start.top) + i64::from(cursor.y) - i64::from(self.cursor.y),
            self.start.right.checked_sub(self.start.left)?,
            self.start.bottom.checked_sub(self.start.top)?,
            work,
        )
    }
}

/// A saved top-left corner belongs to a connected monitor; a changed taskbar is clamped.
pub fn restore(
    home: PhysPoint,
    width: i32,
    height: i32,
    area: PhysRect,
    work: PhysRect,
) -> Option<PhysRect> {
    if !area.contains(home) {
        return None;
    }
    clamped(i64::from(home.x), i64::from(home.y), width, height, work)
}

fn clamped(x: i64, y: i64, width: i32, height: i32, work: PhysRect) -> Option<PhysRect> {
    let width = i64::from(width);
    let height = i64::from(height);
    let max_x = i64::from(work.right) - width;
    let max_y = i64::from(work.bottom) - height;
    if width <= 0 || height <= 0 || max_x < i64::from(work.left) || max_y < i64::from(work.top) {
        return None;
    }
    let left = x.clamp(i64::from(work.left), max_x);
    let top = y.clamp(i64::from(work.top), max_y);
    PhysRect::new(
        left as i32,
        top as i32,
        (left + width) as i32,
        (top + height) as i32,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> PhysRect {
        PhysRect::new(x, y, x + w, y + h).unwrap()
    }

    #[test]
    fn keeps_grab_offset_and_recovers_after_hitting_every_edge() {
        let work = rect(-1000, -500, 1000, 500);
        let start = rect(-600, -300, 200, 200);
        let grab = PhysPoint { x: -550, y: -250 };
        let drag = Drag::new(start, grab);
        assert_eq!(drag.at(grab, work), Some(start));
        for (cursor, expected) in [
            (
                PhysPoint { x: -2000, y: -2000 },
                rect(-1000, -500, 200, 200),
            ),
            (PhysPoint { x: 2000, y: 2000 }, rect(-200, -200, 200, 200)),
            (PhysPoint { x: -450, y: -200 }, rect(-500, -250, 200, 200)),
        ] {
            assert_eq!(drag.at(cursor, work), Some(expected));
        }
        assert_eq!(drag.at(grab, work), Some(start));
    }

    #[test]
    fn impossible_sizes_do_not_panic_or_leave_the_work_area() {
        let drag = Drag::new(rect(0, 0, 220, 220), PhysPoint { x: 0, y: 0 });
        assert_eq!(
            drag.at(
                PhysPoint {
                    x: i32::MIN,
                    y: i32::MAX
                },
                rect(0, 0, 100, 100),
            ),
            None,
        );
        assert_eq!(
            drag.at(
                PhysPoint {
                    x: i32::MAX,
                    y: i32::MIN
                },
                rect(-500, -500, 1000, 1000),
            ),
            Some(rect(280, -500, 220, 220)),
        );
    }

    #[test]
    fn restores_only_connected_monitors_and_clamps_changed_work_areas() {
        let area = rect(-1920, 0, 1920, 1080);
        let work = rect(-1920, 0, 1920, 1040);
        assert_eq!(
            restore(PhysPoint { x: -240, y: 840 }, 220, 220, area, work),
            Some(rect(-240, 820, 220, 220)),
        );
        assert_eq!(
            restore(PhysPoint { x: 100, y: 840 }, 220, 220, area, work),
            None,
        );
        assert_eq!(
            restore(PhysPoint { x: 0, y: 0 }, 220, 220, area, work),
            None,
        );
    }
}
