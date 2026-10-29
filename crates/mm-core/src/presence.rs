//! Session-only visibility and first-close hint state for Mate.

use std::time::{Duration, Instant};

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
}
