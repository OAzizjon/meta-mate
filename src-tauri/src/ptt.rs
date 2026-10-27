//! The push-to-talk driver (spike S2): one thread feeds the keyboard hook's events to
//! `mm_core::ptt::Ptt` and wakes at least every 33 ms for its Tick. One clock for
//! everything: milliseconds since the driver started, for a hook event taken from the
//! event's own `Instant`. While a session is on, Right Ctrl's async key state is the
//! release watchdog: Chrome can swallow a key-up and Windows silently drops a slow
//! hook (CLAUDE.md "Known pitfalls", docs/edge.md section 5).
//!
//! `--ptt-probe` prints the events for 60 s without any window, to try Right Ctrl in
//! Chrome, Telegram and Notepad.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mm_core::ptt::{Key, Ptt, PttEvent, PttInput, PttSettings};
use mm_win32::keyboard::{self, HookError, KeyEvent, KeyboardHook, RIGHT_CTRL};

/// The longest wait between two wake-ups: the Tick and the ~30 Hz watchdog.
const WAKE: Duration = Duration::from_millis(33);
const PROBE: Duration = Duration::from_secs(60);

/// The running driver. Dropping it stops the thread within one wake-up and unhooks.
pub struct PttDriver {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for PttDriver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Installs the hook, then drives `Ptt` on a thread of its own; `on_event` runs there,
/// in order.
pub fn spawn(
    settings: PttSettings,
    mut on_event: impl FnMut(PttEvent) + Send + 'static,
) -> Result<PttDriver, HookError> {
    let started = Instant::now();
    let (events, keys) = mpsc::channel();
    let hook = keyboard::start(events)?;
    let stop = Arc::new(AtomicBool::new(false));
    let thread = std::thread::Builder::new()
        .name("mm-ptt".into())
        .spawn({
            let stop = stop.clone();
            move || {
                run(
                    Feed::new(settings, started),
                    keys,
                    hook,
                    &stop,
                    &mut on_event,
                )
            }
        })
        .map_err(HookError::Thread)?;
    Ok(PttDriver {
        stop,
        thread: Some(thread),
    })
}

fn run(
    mut feed: Feed,
    keys: Receiver<KeyEvent>,
    _hook: KeyboardHook, // unhooks when the loop ends
    stop: &AtomicBool,
    on_event: &mut impl FnMut(PttEvent),
) {
    let mut emit = |events: Vec<PttEvent>| events.into_iter().for_each(&mut *on_event);
    while !stop.load(Ordering::Relaxed) {
        let quiet = match keys.recv_timeout(WAKE) {
            Ok(event) => {
                emit(feed.key(event));
                false
            }
            Err(RecvTimeoutError::Timeout) => true,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        emit(feed.tick(Instant::now()));
        // Windows updates the async key state only after the hook returns: polled right
        // after a hook event it can still say "up" and drop a press. A quiet wait has
        // let it catch up.
        if quiet && feed.active() {
            let down = keyboard::right_ctrl_down();
            let now = Instant::now();
            // A key-up the hook saw is queued before the async state shows it: it goes
            // first, and the next quiet wait polls again.
            match keys.try_recv() {
                Ok(event) => emit(feed.key(event)),
                Err(_) => emit(feed.poll(down, now)),
            }
        }
    }
}

/// Hook events and wake-ups as `PttInput`s on one clock, and whether a session is on
/// (between Start and Stop or Cancel). Pure: the caller passes every `Instant`.
struct Feed {
    ptt: Ptt,
    started: Instant,
    active: bool,
}

impl Feed {
    fn new(settings: PttSettings, started: Instant) -> Self {
        Self {
            ptt: Ptt::new(settings),
            started,
            active: false,
        }
    }

    fn active(&self) -> bool {
        self.active
    }

    fn key(&mut self, event: KeyEvent) -> Vec<PttEvent> {
        let key = if event.vk == RIGHT_CTRL {
            Key::RightCtrl
        } else {
            Key::Other(event.vk)
        };
        let (injected, now_ms) = (event.injected, self.ms(event.at));
        self.input(if event.down {
            PttInput::KeyDown {
                key,
                injected,
                now_ms,
            }
        } else {
            PttInput::KeyUp {
                key,
                injected,
                now_ms,
            }
        })
    }

    fn tick(&mut self, now: Instant) -> Vec<PttEvent> {
        let now_ms = self.ms(now);
        self.input(PttInput::Tick { now_ms })
    }

    fn poll(&mut self, ptt_key_down: bool, now: Instant) -> Vec<PttEvent> {
        let now_ms = self.ms(now);
        self.input(PttInput::Poll {
            ptt_key_down,
            now_ms,
        })
    }

    fn input(&mut self, input: PttInput) -> Vec<PttEvent> {
        let events = self.ptt.input(input);
        for event in &events {
            self.active = matches!(event, PttEvent::Start { .. });
        }
        events
    }

    /// Milliseconds since the driver started; an earlier `Instant` counts as 0.
    fn ms(&self, at: Instant) -> u64 {
        u64::try_from(at.saturating_duration_since(self.started).as_millis()).unwrap_or(u64::MAX)
    }
}

/// `--ptt-probe`: no window; prints every `PttEvent` with the milliseconds since the
/// start for 60 s, then exits 0 (1 when the hook cannot be installed). None without
/// the flag.
pub fn probe_if_requested(args: &[String]) -> Option<i32> {
    if !args.iter().any(|arg| arg == "--ptt-probe") {
        return None;
    }
    println!("PTT probe: hold Right Ctrl in any app; ends in 60 s");
    let started = Instant::now();
    let driver = spawn(PttSettings::default(), move |event| {
        println!("{:>6} ms  {event:?}", started.elapsed().as_millis());
    });
    let driver = match driver {
        Ok(driver) => driver,
        Err(error) => {
            eprintln!("PTT probe: {error}");
            return Some(1);
        }
    };
    std::thread::sleep(PROBE);
    drop(driver);
    Some(0)
}

#[cfg(test)]
mod tests {
    use mm_core::ptt::StopReason;

    use super::*;

    /// Any other key; only its code matters.
    const A: u32 = 0x41;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    fn key(vk: u32, down: bool, at: Instant) -> KeyEvent {
        KeyEvent {
            vk,
            down,
            injected: false,
            at,
        }
    }

    fn injected(vk: u32, down: bool, at: Instant) -> KeyEvent {
        KeyEvent {
            injected: true,
            ..key(vk, down, at)
        }
    }

    fn nothing() -> Vec<PttEvent> {
        Vec::new()
    }

    fn start(generation: u64) -> Vec<PttEvent> {
        vec![PttEvent::Start { generation }]
    }

    fn stop(generation: u64, reason: StopReason) -> Vec<PttEvent> {
        vec![PttEvent::Stop { generation, reason }]
    }

    #[test]
    fn hold_then_release_starts_and_stops() {
        let t0 = Instant::now();
        let mut feed = Feed::new(PttSettings::default(), t0);
        assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 0))), nothing());
        assert_eq!(feed.tick(at(t0, 249)), nothing());
        assert!(!feed.active());
        assert_eq!(feed.tick(at(t0, 250)), start(1));
        assert!(feed.active());
        // Auto-repeat and a poll that sees the key held change nothing.
        assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 280))), nothing());
        assert_eq!(feed.poll(true, at(t0, 313)), nothing());
        assert_eq!(
            feed.key(key(RIGHT_CTRL, false, at(t0, 400))),
            stop(1, StopReason::Released)
        );
        assert!(!feed.active());
    }

    #[test]
    fn another_key_during_the_hold_starts_nothing() {
        let t0 = Instant::now();
        let mut feed = Feed::new(PttSettings::default(), t0);
        assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 0))), nothing());
        assert_eq!(feed.key(key(A, true, at(t0, 100))), nothing());
        assert_eq!(feed.tick(at(t0, 300)), nothing());
        assert_eq!(feed.key(key(A, false, at(t0, 350))), nothing());
        assert_eq!(feed.tick(at(t0, 400)), nothing());
        assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 500))), nothing());
        assert!(!feed.active());
    }

    #[test]
    fn another_key_after_the_start_cancels() {
        let t0 = Instant::now();
        let mut feed = Feed::new(PttSettings::default(), t0);
        feed.key(key(RIGHT_CTRL, true, at(t0, 0)));
        assert_eq!(feed.tick(at(t0, 250)), start(1));
        assert_eq!(
            feed.key(key(A, true, at(t0, 300))),
            vec![PttEvent::Cancel { generation: 1 }]
        );
        assert!(!feed.active());
    }

    #[test]
    fn lost_key_up_is_caught_by_the_poll() {
        let t0 = Instant::now();
        let mut feed = Feed::new(PttSettings::default(), t0);
        feed.key(key(RIGHT_CTRL, true, at(t0, 0)));
        assert_eq!(feed.tick(at(t0, 250)), start(1));
        assert_eq!(feed.poll(true, at(t0, 283)), nothing());
        // The key-up never reaches the hook; only the async key state says "up".
        assert_eq!(feed.tick(at(t0, 316)), nothing());
        assert_eq!(feed.poll(false, at(t0, 316)), stop(1, StopReason::Watchdog));
        assert!(!feed.active());
        // The next press works again.
        feed.key(key(RIGHT_CTRL, true, at(t0, 1_000)));
        assert_eq!(feed.tick(at(t0, 1_250)), start(2));
    }

    #[test]
    fn injected_events_are_ignored() {
        let t0 = Instant::now();
        let mut feed = Feed::new(PttSettings::default(), t0);
        assert_eq!(feed.key(injected(RIGHT_CTRL, true, at(t0, 0))), nothing());
        assert_eq!(feed.tick(at(t0, 300)), nothing());
        assert!(!feed.active());
        feed.key(key(RIGHT_CTRL, true, at(t0, 400)));
        assert_eq!(feed.tick(at(t0, 650)), start(1));
        assert_eq!(
            feed.key(injected(RIGHT_CTRL, false, at(t0, 700))),
            nothing()
        );
        assert_eq!(feed.key(injected(A, true, at(t0, 710))), nothing());
        assert!(feed.active());
        assert_eq!(
            feed.key(key(RIGHT_CTRL, false, at(t0, 800))),
            stop(1, StopReason::Released)
        );
    }

    #[test]
    fn one_clock_from_the_driver_start() {
        let t0 = Instant::now();
        let feed = Feed::new(PttSettings::default(), at(t0, 100));
        assert_eq!(feed.ms(t0), 0);
        assert_eq!(feed.ms(at(t0, 350)), 250);
    }
}
