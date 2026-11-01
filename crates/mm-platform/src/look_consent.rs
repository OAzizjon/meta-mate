//! Bounded, session-only permission to look at a particular window.

use crate::TargetWindow;

const MAX_WINDOWS: usize = 32;

#[derive(Default)]
pub struct LookConsent {
    windows: Vec<TargetWindow>,
}

impl LookConsent {
    pub const fn new() -> Self {
        Self {
            windows: Vec::new(),
        }
    }

    /// Drop closed windows before checking an exact handle/process pair.
    /// The caller supplies a live OS identity check, independent of visibility.
    pub fn granted(
        &mut self,
        target: TargetWindow,
        mut exists: impl FnMut(TargetWindow) -> bool,
    ) -> bool {
        self.windows.retain(|&window| exists(window));
        self.windows.contains(&target)
    }

    /// Only an explicit yes is remembered; refusals never create permission.
    pub fn answer(&mut self, target: TargetWindow, yes: bool) {
        if !yes || self.windows.contains(&target) {
            return;
        }
        if self.windows.len() == MAX_WINDOWS {
            self.windows.remove(0);
        }
        self.windows.push(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(handle: u64, process_id: u32) -> TargetWindow {
        TargetWindow { handle, process_id }
    }

    #[test]
    fn grant_applies_only_to_the_same_live_window_and_process() {
        let mut consent = LookConsent::new();
        let target = window(1, 10);
        assert!(!consent.granted(target, |_| true));
        consent.answer(target, true);
        assert!(consent.granted(target, |_| true));
        assert!(!consent.granted(window(2, 10), |_| true));
        assert!(!consent.granted(window(1, 11), |_| true));
        assert!(!consent.granted(target, |_| false));
        // Once observed closed, even the old numeric identity has no permission.
        assert!(!consent.granted(target, |_| true));
    }

    #[test]
    fn refusal_is_not_remembered_and_a_new_session_has_no_grants() {
        let target = window(1, 10);
        let mut consent = LookConsent::default();
        consent.answer(target, false);
        assert!(!consent.granted(target, |_| true));
        consent.answer(target, true);
        assert!(!LookConsent::new().granted(target, |_| true));
    }

    #[test]
    fn oldest_grant_is_evicted_and_duplicates_do_not_consume_capacity() {
        let mut consent = LookConsent::new();
        for handle in 1..=32 {
            consent.answer(window(handle, 10), true);
        }
        consent.answer(window(1, 10), true);
        assert_eq!(consent.windows.len(), MAX_WINDOWS);
        consent.answer(window(33, 10), true);
        assert_eq!(consent.windows.len(), MAX_WINDOWS);
        assert!(!consent.granted(window(1, 10), |_| true));
        assert!(consent.granted(window(2, 10), |_| true));
        assert!(consent.granted(window(33, 10), |_| true));
    }
}
