//! Session-only visibility and first-close hint state for Mate.

use std::time::{Duration, Instant};

use crate::geom::PhysRect;

pub const HIDE_DURATION: Duration = Duration::from_secs(60 * 60);

#[derive(Default)]
pub struct Presence {
    hidden_until: Option<Instant>,
    home_closed: bool,
}

impl Presence {
    pub fn close_home(&mut self) -> bool {
        let first = !self.home_closed;
        self.home_closed = true;
        first
    }

    pub fn hide(&mut self, now: Instant) {
        self.hidden_until = Some(now + HIDE_DURATION);
    }

    /// A delayed window hide belongs only to this request, and never survives a wake.
    pub fn hide_is_current(&self, started: Instant) -> bool {
        self.hidden_until == Some(started + HIDE_DURATION)
    }

    /// A PTT press wakes Mate early; old timers must then do nothing.
    pub fn wake(&mut self) -> bool {
        self.hidden_until.take().is_some()
    }

    pub fn wake_if_due(&mut self, now: Instant) -> bool {
        if self.hidden_until.is_some_and(|deadline| now >= deadline) {
            self.wake()
        } else {
            false
        }
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden_until.is_some()
    }
}

/// True when no pixel of Mate's `rect` is on any of the `monitors`' areas: a monitor
/// unplugged while he slept would wake him where nobody can see him.
pub fn lost(rect: PhysRect, monitors: &[PhysRect]) -> bool {
    !crate::redaction::overlaps_any(rect, monitors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_hint_is_once_per_session() {
        let mut state = Presence::default();
        assert!(state.close_home());
        assert!(!state.close_home());
        assert!(!state.close_home());
    }

    #[test]
    fn hide_expires_after_one_hour_or_ptt() {
        let now = Instant::now();
        let mut state = Presence::default();
        assert!(!state.wake());
        state.hide(now);
        assert!(!state.wake_if_due(now + HIDE_DURATION - Duration::from_millis(1)));
        assert!(state.wake_if_due(now + HIDE_DURATION));
        assert!(!state.wake());
        state.hide(now);
        assert!(state.wake());
        assert!(!state.wake_if_due(now + HIDE_DURATION));
    }

    #[test]
    fn old_timer_cannot_end_a_new_hide() {
        let now = Instant::now();
        let mut state = Presence::default();
        state.hide(now);
        assert!(state.wake());
        state.hide(now + Duration::from_secs(30));
        assert!(!state.wake_if_due(now + HIDE_DURATION));
        assert!(state.wake_if_due(now + HIDE_DURATION + Duration::from_secs(30)));
    }

    #[test]
    fn delayed_hide_is_cancelled_by_wake_or_replacement() {
        let now = Instant::now();
        let mut state = Presence::default();
        state.hide(now);
        assert!(state.hide_is_current(now));
        assert!(!state.wake_if_due(now + Duration::from_secs(3)));
        assert!(state.wake());
        assert!(!state.hide_is_current(now));
        state.hide(now + Duration::from_secs(1));
        assert!(!state.hide_is_current(now));
        assert!(state.hide_is_current(now + Duration::from_secs(1)));
    }

    #[test]
    fn hidden_until_woken() {
        let now = Instant::now();
        let mut state = Presence::default();
        assert!(!state.is_hidden());
        state.hide(now);
        assert!(state.is_hidden());
        assert!(!state.wake_if_due(now));
        assert!(state.is_hidden());
        assert!(state.wake());
        assert!(!state.is_hidden());
    }

    #[test]
    fn lost_only_off_every_monitor() {
        let rect = |left, top, right, bottom| PhysRect::new(left, top, right, bottom).unwrap();
        let monitors = [rect(0, 0, 1920, 1080), rect(1920, 0, 3840, 1080)];
        assert!(!lost(rect(1600, 800, 1820, 1020), &monitors));
        // Across the seam, or one pixel left on a monitor: still seen.
        assert!(!lost(rect(1800, 100, 2020, 320), &monitors));
        assert!(!lost(rect(3839, 100, 4059, 320), &monitors));
        // The second monitor unplugged.
        assert!(lost(rect(2200, 100, 2420, 320), &monitors[..1]));
        // Touching an edge from outside is not a pixel on it.
        assert!(lost(rect(3840, 100, 4060, 320), &monitors));
        assert!(lost(rect(0, -220, 220, 0), &monitors));
        assert!(lost(rect(0, 0, 220, 220), &[]));
    }
}
