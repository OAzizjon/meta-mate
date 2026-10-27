use crate::geom::PhysPoint;

use super::TeacherInput;

#[derive(Debug, Clone, Copy)]
struct Press {
    at: PhysPoint,
    now_ms: u64,
    injected: bool,
}

/// Converts button edges into clicks without completing any teacher step itself.
#[derive(Debug)]
pub struct ClickRecognizer {
    double_ms: u32,
    cx: i32,
    cy: i32,
    now_ms: u64,
    pressed: [Option<Press>; 2],
    first: Option<Press>,
}

impl ClickRecognizer {
    /// `cx` and `cy` are the full dimensions of the double-click rectangle.
    /// Non-positive dimensions disable pairing.
    pub fn new(double_ms: u32, cx: i32, cy: i32) -> Self {
        Self {
            double_ms,
            cx,
            cy,
            now_ms: 0,
            pressed: [None; 2],
            first: None,
        }
    }

    /// Feed edges in delivery order; backward timestamps are clamped.
    /// Right and middle share the non-left slot in this boolean protocol.
    pub fn button(
        &mut self,
        left: bool,
        down: bool,
        at: PhysPoint,
        injected: bool,
        now_ms: u64,
    ) -> Vec<TeacherInput> {
        self.now_ms = self.now_ms.max(now_ms);
        let slot = &mut self.pressed[usize::from(left)];
        if down {
            if let Some(press) = slot {
                // Repeated DOWNs do not move the original press or lose provenance.
                press.injected |= injected;
            } else {
                *slot = Some(Press {
                    at,
                    now_ms: self.now_ms,
                    injected,
                });
            }
            return Vec::new();
        }
        let Some(mut press) = slot.take() else {
            return Vec::new();
        };
        press.injected |= injected;
        let mut out = vec![TeacherInput::Click {
            at,
            left,
            injected: press.injected,
            now_ms: self.now_ms,
        }];
        if left {
            let first = self.first.take();
            if let Some(first) = first.filter(|first| self.pairs(*first, press)) {
                out.push(TeacherInput::DoubleClick {
                    at,
                    left: true,
                    injected: first.injected || press.injected,
                    now_ms: self.now_ms,
                });
            } else {
                self.first = Some(press);
            }
        }
        out
    }

    fn pairs(&self, first: Press, second: Press) -> bool {
        // Widen before subtracting: virtual-screen positions can be negative.
        let within = |a: i32, b: i32, size: i32| {
            let delta = i64::from(b) - i64::from(a);
            let size = i64::from(size);
            size > 0 && delta >= -(size / 2) && delta < size - size / 2
        };
        second.now_ms.saturating_sub(first.now_ms) <= u64::from(self.double_ms)
            && within(first.at.x, second.at.x, self.cx)
            && within(first.at.y, second.at.y, self.cy)
    }
}
