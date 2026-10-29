//! The push-to-talk driver (spike S2): one thread feeds the keyboard hook's events to
//! `mm_core::ptt::Ptt` and wakes every 33 ms while a press or recording is pending.
//! Idle watchdog recovery runs every 250 ms. One clock for
//! everything: milliseconds since the driver started, for a hook event taken from the
//! event's own `Instant`. While a session is on, the talk key's async key state is the
//! release watchdog: Chrome can swallow a key-up and Windows silently drops a slow
//! hook (CLAUDE.md "Known pitfalls", docs/edge.md section 5).
//!
//! `--ptt-probe` prints the events for 60 s without any window, to try the talk key in
//! Chrome, Telegram and Notepad.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mm_core::ptt::{Key, Mode, Ptt, PttEvent, PttInput, PttSettings, StopReason};
use mm_win32::keyboard::{self, HookError, KeyEvent, KeyboardHook, RIGHT_CTRL};

/// The active wait between two wake-ups: the Tick and the ~30 Hz watchdog.
const WAKE: Duration = Duration::from_millis(33);
const IDLE_WAKE: Duration = Duration::from_millis(250);
const PROBE: Duration = Duration::from_secs(60);
/// The talk key held this long while the hook reported nothing of it: Windows removed the
/// hook (a slow one is dropped silently), so it is installed again, at most every
/// `REINSTALL_EVERY`.
const HOOK_SILENT: Duration = Duration::from_millis(500);
const REINSTALL_EVERY: Duration = Duration::from_secs(5);

// Published only by a running driver; subscribers never keep the driver alive.
static KEY_TAP: Mutex<Weak<KeyTap>> = Mutex::new(Weak::new());
static TOGGLE_MODE: AtomicBool = AtomicBool::new(false);
static PTT_KEY: AtomicU32 = AtomicU32::new(RIGHT_CTRL);
static CLICKS: Mutex<Option<Sender<DriverInput>>> = Mutex::new(None);

enum DriverInput {
    Key(KeyEvent),
    Click(Instant),
    Silence(u64),
    Wake,
}

/// Native drag release uses the same serialized driver as the keyboard, without focus.
pub(crate) fn click() {
    if let Ok(sender) = CLICKS.lock()
        && let Some(sender) = sender.as_ref()
    {
        let _ = sender.send(DriverInput::Click(Instant::now()));
    }
}

pub fn set_ptt_key(key: crate::settings::PttKey) {
    let vk = key.virtual_key();
    keyboard::set_ptt_key(vk);
    PTT_KEY.store(vk, Ordering::Relaxed);
}
const TAP_MS: u64 = 400;
const TOGGLE_MAX_MS: u64 = 30_000;

/// The settings owner calls this after loading or saving settings. Each genuine
/// press samples it; changing settings does not reinterpret a key already held.
pub fn set_ptt_mode(mode: crate::settings::Ptt) {
    TOGGLE_MODE.store(
        matches!(mode, crate::settings::Ptt::Toggle),
        Ordering::Relaxed,
    );
}

#[derive(Default)]
struct KeyTap {
    subscriber: Mutex<Option<(Weak<()>, Sender<KeyEvent>)>>,
}

/// Dropping a lesson disconnects its event queue without affecting a newer lesson.
pub(crate) struct KeyTapGuard {
    tap: Weak<KeyTap>,
    identity: Arc<()>,
}

impl Drop for KeyTapGuard {
    fn drop(&mut self) {
        if let Some(tap) = self.tap.upgrade()
            && let Ok(mut subscriber) = tap.subscriber.lock()
            && subscriber
                .as_ref()
                .is_some_and(|(identity, _)| identity.ptr_eq(&Arc::downgrade(&self.identity)))
        {
            *subscriber = None;
        }
    }
}

impl KeyTap {
    fn subscribe(self: &Arc<Self>) -> Option<(KeyTapGuard, Receiver<KeyEvent>)> {
        let mut subscriber = self.subscriber.lock().ok()?;
        let (events, received) = mpsc::channel();
        let identity = Arc::new(());
        *subscriber = Some((Arc::downgrade(&identity), events));
        Some((
            KeyTapGuard {
                tap: Arc::downgrade(self),
                identity,
            },
            received,
        ))
    }

    // Runs on mm-ptt, never inside the low-level hook. An unbounded send cannot
    // wait for the teacher to consume an event; no character conversion or logging.
    fn send(&self, event: KeyEvent) {
        if let Ok(mut subscriber) = self.subscriber.lock()
            && let Some((_, events)) = subscriber.as_ref()
            && events.send(event).is_err()
        {
            *subscriber = None;
        }
    }
}

/// Subscribe to the existing PTT hook; fail closed if no driver is running.
pub(crate) fn subscribe_keys() -> Option<(KeyTapGuard, Receiver<KeyEvent>)> {
    KEY_TAP.lock().ok()?.upgrade()?.subscribe()
}

mod driver;
mod feed;

pub(crate) use driver::spawn_timed;
#[allow(unused_imports)] // `spawn` was public before the split; kept for callers.
pub use driver::{PttDriver, Silencer, probe_if_requested, spawn};
use feed::Feed;

#[cfg(test)]
mod tests;
