//! Recent eligible apps, without any platform queries or activation.

use crate::TargetWindow;

/// The last three distinct targets; callers decide whether each is still eligible.
#[derive(Default)]
pub struct RecentTargets {
    windows: Vec<TargetWindow>,
}

impl RecentTargets {
    /// Newest last; repeated samples keep the other recent apps available.
    pub fn remember(&mut self, target: TargetWindow) {
        self.windows.retain(|old| *old != target);
        self.windows.push(target);
        if self.windows.len() > 3 {
            self.windows.remove(0);
        }
    }

    /// Recheck candidates at use time: a recent app may have closed or minimized.
    pub fn newest_valid(
        &self,
        mut valid: impl FnMut(TargetWindow) -> bool,
    ) -> Option<TargetWindow> {
        self.windows
            .iter()
            .rev()
            .copied()
            .find(|&target| valid(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(handle: u64) -> TargetWindow {
        TargetWindow {
            handle,
            process_id: 1,
        }
    }

    #[test]
    fn repeated_samples_keep_three_distinct_apps_in_recency_order() {
        let mut targets = RecentTargets::default();
        for handle in [1, 2, 3, 3, 2, 4] {
            targets.remember(window(handle));
        }
        assert_eq!(targets.windows, vec![window(3), window(2), window(4)]);
    }

    #[test]
    fn newest_invalid_app_falls_back_and_no_valid_app_returns_none() {
        let mut targets = RecentTargets::default();
        assert_eq!(targets.newest_valid(|_| true), None);
        for handle in [1, 2, 3] {
            targets.remember(window(handle));
        }
        assert_eq!(targets.newest_valid(|_| true), Some(window(3)));
        assert_eq!(
            targets.newest_valid(|target| target.handle != 3),
            Some(window(2))
        );
        assert_eq!(
            targets.newest_valid(|target| target.handle == 1),
            Some(window(1))
        );
        assert_eq!(targets.newest_valid(|_| false), None);
    }
}
