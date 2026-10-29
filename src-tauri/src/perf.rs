//! Debug-only wake rates; no window titles, input or other user data is logged.

use std::time::Instant;

/// Counts wakes and logs their rate at most once every five seconds.
pub(crate) struct Wakes {
    name: &'static str,
    since: Instant,
    count: u64,
}

impl Wakes {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            since: Instant::now(),
            count: 0,
        }
    }

    pub(crate) fn wake(&mut self) {
        self.count += 1;
        let elapsed = self.since.elapsed();
        if elapsed.as_secs() >= 5 {
            eprintln!(
                "perf: {}: {:.1} wakes/s ({} wakes / {:.2}s)",
                self.name,
                self.count as f64 / elapsed.as_secs_f64(),
                self.count,
                elapsed.as_secs_f64()
            );
            self.count = 0;
            self.since = Instant::now();
        }
    }
}
