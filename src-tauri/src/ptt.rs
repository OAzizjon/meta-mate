//! The push-to-talk driver (spike S2): one thread feeds the keyboard hook's events to
//! `mm_core::ptt::Ptt` and wakes at least every 33 ms for its Tick. One clock for
//! everything: milliseconds since the driver started, for a hook event taken from the
//! event's own `Instant`. While a session is on, Right Ctrl's async key state is the
//! release watchdog: Chrome can swallow a key-up and Windows silently drops a slow
//! hook (CLAUDE.md "Known pitfalls", docs/edge.md section 5).
//!
//! `--ptt-probe` prints the events for 60 s without any window, to try Right Ctrl in
//! Chrome, Telegram and Notepad.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mm_core::ptt::{Key, Mode, Ptt, PttEvent, PttInput, PttSettings, StopReason};
use mm_win32::keyboard::{self, HookError, KeyEvent, KeyboardHook, RIGHT_CTRL};

/// The longest wait between two wake-ups: the Tick and the ~30 Hz watchdog.
const WAKE: Duration = Duration::from_millis(33);
const PROBE: Duration = Duration::from_secs(60);
/// Right Ctrl held this long while the hook reported nothing of it: Windows removed the
/// hook (a slow one is dropped silently), so it is installed again, at most every
/// `REINSTALL_EVERY`.
const HOOK_SILENT: Duration = Duration::from_millis(500);
const REINSTALL_EVERY: Duration = Duration::from_secs(5);

// Published only by a running driver; subscribers never keep the driver alive.
static KEY_TAP: Mutex<Weak<KeyTap>> = Mutex::new(Weak::new());
static TOGGLE_MODE: AtomicBool = AtomicBool::new(false);
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

/// The running driver. Dropping it stops the thread within one wake-up and unhooks.
pub struct PttDriver {
    stop: Arc<AtomicBool>,
    silence: Sender<u64>,
    thread: Option<JoinHandle<()>>,
}

impl PttDriver {
    /// A handle the microphone's thread can keep: it ends only its own recording.
    pub fn silencer(&self) -> Silencer {
        Silencer(self.silence.clone())
    }
}

/// Ends one hands-free recording ("press, talk, pause") after the speech ends.
#[derive(Clone)]
pub struct Silencer(Sender<u64>);

impl Silencer {
    /// End only this recording, so a delayed VAD result cannot stop a newer one.
    pub fn silence(&self, generation: u64) {
        let _ = self.0.send(generation);
    }
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
    spawn_timed(settings, move |event, _| on_event(event))
}

/// Carries the genuine key-down time, before hold detection or queueing delays.
pub(crate) fn spawn_timed(
    settings: PttSettings,
    mut on_event: impl FnMut(PttEvent, Option<Instant>) + Send + 'static,
) -> Result<PttDriver, HookError> {
    // PttSettings already supplies the startup mode; preserve the spawn API used
    // by voice.rs. Later changes arrive through set_ptt_mode.
    TOGGLE_MODE.store(settings.mode == Mode::Toggle, Ordering::Relaxed);
    let started = Instant::now();
    let (events, keys) = mpsc::channel();
    let hook = keyboard::start(events.clone())?;
    let tap = Arc::new(KeyTap::default());
    let published = Arc::downgrade(&tap);
    let stop = Arc::new(AtomicBool::new(false));
    let (silence, silences) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("mm-ptt".into())
        .spawn({
            let stop = stop.clone();
            move || {
                run(
                    Feed::new(settings, started),
                    (keys, silences),
                    events,
                    hook,
                    tap,
                    &stop,
                    &mut on_event,
                )
            }
        })
        .map_err(HookError::Thread)?;
    if let Ok(mut current) = KEY_TAP.lock() {
        *current = published;
    }
    Ok(PttDriver {
        stop,
        silence,
        thread: Some(thread),
    })
}

fn run(
    mut feed: Feed,
    inputs: (Receiver<KeyEvent>, Receiver<u64>),
    events: Sender<KeyEvent>,
    mut hook: KeyboardHook, // unhooks when the loop ends
    tap: Arc<KeyTap>,
    stop: &AtomicBool,
    on_event: &mut impl FnMut(PttEvent, Option<Instant>),
) {
    let (keys, silences) = inputs;
    let mut emit = |events: Vec<PttEvent>, pressed_at| {
        for event in events {
            on_event(event, pressed_at);
        }
    };
    // The last Right Ctrl event the hook delivered, and the last reinstall.
    let mut last_ctrl = Instant::now();
    let mut reinstalled = Instant::now()
        .checked_sub(REINSTALL_EVERY)
        .unwrap_or_else(Instant::now);
    while !stop.load(Ordering::Relaxed) {
        match keys.recv_timeout(WAKE) {
            Ok(event) => {
                tap.send(event);
                if event.vk == RIGHT_CTRL {
                    last_ctrl = event.at;
                }
                let events = feed.driver_key(event);
                emit(events, feed.pressed_at);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        // Not `events`: that name is the hook's sender, needed for a reinstall below.
        let ticked = feed.tick(now);
        emit(ticked, feed.pressed_at);
        for generation in silences.try_iter() {
            let events = feed.silence(generation);
            emit(events, feed.pressed_at);
        }
        // Windows updates the async key state only after the hook returns: polled right
        // after a Right Ctrl event it can still read the old state. One WAKE later it has
        // caught up; other keys' events do not matter.
        let silent = now.saturating_duration_since(last_ctrl);
        if silent < WAKE {
            continue;
        }
        let down = keyboard::right_ctrl_down();
        if feed.needs_poll() {
            // An event queued meanwhile goes first; the next wake polls again.
            match keys.try_recv() {
                Ok(event) => {
                    tap.send(event);
                    if event.vk == RIGHT_CTRL {
                        last_ctrl = event.at;
                    }
                    let events = feed.driver_key(event);
                    emit(events, feed.pressed_at);
                }
                Err(_) => {
                    let events = feed.poll(down, now);
                    emit(events, feed.pressed_at);
                }
            }
        } else if down
            && silent >= HOOK_SILENT
            && now.saturating_duration_since(reinstalled) >= REINSTALL_EVERY
        {
            // Held, yet the hook never said so: Windows removed it.
            reinstalled = now;
            if let Ok(fresh) = keyboard::start(events.clone()) {
                hook = fresh;
            }
        }
    }
    drop(hook);
}

/// Hook events and wake-ups as `PttInput`s on one clock, and whether a session is on
/// (between Start and Stop or Cancel). Pure: the caller passes every `Instant`.
struct Feed {
    pressed_at: Option<Instant>,
    ptt: Ptt,
    settings: PttSettings,
    started: Instant,
    active: bool,
    generation: u64,
    active_since: u64,
    now_ms: u64,
    /// The core validates chords, injected events and repeats. In toggle mode
    /// its zero-delay hold is a gesture, not permission to open the microphone.
    gesture: Option<u64>,
}

impl Feed {
    fn new(settings: PttSettings, started: Instant) -> Self {
        Self {
            pressed_at: None,
            ptt: Ptt::new(Self::core_settings(settings)),
            settings,
            started,
            active: false,
            generation: 0,
            active_since: 0,
            now_ms: 0,
            gesture: None,
        }
    }

    fn core_settings(mut settings: PttSettings) -> PttSettings {
        if settings.mode == Mode::Toggle {
            settings.mode = Mode::Hold;
            settings.hold_ms = 0;
            settings.max_ms = u64::MAX;
        }
        settings
    }

    fn driver_key(&mut self, event: KeyEvent) -> Vec<PttEvent> {
        let mode = if TOGGLE_MODE.load(Ordering::Relaxed) {
            Mode::Toggle
        } else {
            Mode::Hold
        };
        self.key_in_mode(event, mode)
    }

    fn key_in_mode(&mut self, event: KeyEvent, mode: Mode) -> Vec<PttEvent> {
        let mut out = Vec::new();
        if event.vk == RIGHT_CTRL
            && event.down
            && !event.injected
            && !self.ptt.key_pressed()
            && self.settings.mode != mode
        {
            self.end(StopReason::Cancelled, &mut out);
            self.settings.mode = mode;
            self.gesture = None;
            let _ = self
                .ptt
                .input(PttInput::SettingsChanged(Self::core_settings(
                    self.settings,
                )));
        }
        out.extend(self.key(event));
        out
    }

    fn silence(&mut self, generation: u64) -> Vec<PttEvent> {
        let mut out = Vec::new();
        if self.active && self.settings.mode == Mode::Toggle && generation == self.generation {
            self.end(StopReason::Toggled, &mut out);
            self.gesture = None;
        }
        out
    }

    #[cfg(test)]
    fn active(&self) -> bool {
        self.active
    }

    /// Polling is due during a session and while a press the hook reported is pending
    /// (after TooLong or a chord too): a lost key-up must never wedge the next press.
    fn needs_poll(&self) -> bool {
        self.active || self.ptt.key_pressed()
    }

    fn key(&mut self, event: KeyEvent) -> Vec<PttEvent> {
        if event.vk == RIGHT_CTRL && event.down && !event.injected && !self.ptt.key_pressed() {
            self.pressed_at = Some(event.at);
        }
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
        let time = match input {
            PttInput::KeyDown { injected: true, .. } | PttInput::KeyUp { injected: true, .. } => {
                return Vec::new();
            }
            PttInput::KeyDown { now_ms, .. }
            | PttInput::KeyUp { now_ms, .. }
            | PttInput::Tick { now_ms }
            | PttInput::Poll { now_ms, .. } => now_ms,
            PttInput::SettingsChanged(_) => self.now_ms,
        };
        self.now_ms = self.now_ms.max(time);
        let events = self.ptt.input(input);
        let mut out = Vec::new();
        if self.settings.mode == Mode::Hold {
            for event in events {
                match event {
                    PttEvent::Start { .. } => self.begin(&mut out),
                    PttEvent::Stop { reason, .. } => self.end(reason, &mut out),
                    PttEvent::Cancel { .. } => self.cancel(&mut out),
                }
            }
            return out;
        }
        // The logical recording has its own deadline, including time with the
        // key released. Expiration/silence also suppress the pending gesture.
        if self.active
            && self.now_ms.saturating_sub(self.active_since)
                >= TOGGLE_MAX_MS.min(self.settings.max_ms)
        {
            self.end(StopReason::TooLong, &mut out);
            self.gesture = None;
        }
        for event in events {
            match event {
                PttEvent::Start { .. } => self.gesture = Some(self.now_ms),
                PttEvent::Cancel { .. } => {
                    self.gesture = None;
                    self.cancel(&mut out);
                }
                PttEvent::Stop { reason, .. } => {
                    if let Some(pressed) = self.gesture.take() {
                        let short = self.now_ms.saturating_sub(pressed) < TAP_MS;
                        if self.active && (reason != StopReason::Watchdog || !short) {
                            let reason = if short { StopReason::Toggled } else { reason };
                            self.end(reason, &mut out);
                        } else if reason == StopReason::Released && short {
                            self.begin(&mut out);
                        }
                    }
                }
            }
        }
        if self
            .gesture
            .is_some_and(|pressed| self.now_ms.saturating_sub(pressed) >= TAP_MS)
            && !self.active
        {
            self.begin(&mut out);
        }
        out
    }

    fn begin(&mut self, out: &mut Vec<PttEvent>) {
        if let Some(generation) = self.generation.checked_add(1) {
            self.generation = generation;
            self.active = true;
            self.active_since = self.now_ms;
            out.push(PttEvent::Start { generation });
        }
    }

    fn end(&mut self, reason: StopReason, out: &mut Vec<PttEvent>) {
        if self.active {
            self.active = false;
            out.push(PttEvent::Stop {
                generation: self.generation,
                reason,
            });
        }
    }

    fn cancel(&mut self, out: &mut Vec<PttEvent>) {
        if self.active {
            self.active = false;
            out.push(PttEvent::Cancel {
                generation: self.generation,
            });
        }
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
    use super::*;

    /// Any other key; only its code matters.
    const A: u32 = 0x41;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    fn key(vk: u32, down: bool, at: Instant) -> KeyEvent {
        KeyEvent {
            foreground: None,
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

    #[test]
    fn tap_preserves_injected_flags_and_drop_disconnects() {
        let tap = Arc::new(KeyTap::default());
        let (guard, received) = tap.subscribe().unwrap();
        let event = injected(A, true, Instant::now());
        tap.send(event);
        assert_eq!(received.try_recv(), Ok(event));
        drop(guard);
        assert_eq!(received.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }

    #[test]
    fn press_time_precedes_hold_start_and_survives_repeats_and_release() {
        let t0 = Instant::now();
        let mut feed = Feed::new(PttSettings::default(), t0);
        assert_eq!(feed.key(key(RIGHT_CTRL, true, t0)), nothing());
        assert_eq!(feed.tick(at(t0, 250)), start(1));
        feed.key(key(RIGHT_CTRL, true, at(t0, 300)));
        feed.key(injected(RIGHT_CTRL, true, at(t0, 350)));
        assert_eq!(feed.pressed_at, Some(t0));
        feed.key(key(RIGHT_CTRL, false, at(t0, 400)));
        assert_eq!(feed.pressed_at, Some(t0));
        feed.key(key(RIGHT_CTRL, true, at(t0, 500)));
        assert_eq!(feed.pressed_at, Some(at(t0, 500)));
    }

    #[test]
    fn old_guard_does_not_cancel_replacement_subscription() {
        let tap = Arc::new(KeyTap::default());
        let (old, old_received) = tap.subscribe().unwrap();
        let (_new, received) = tap.subscribe().unwrap();
        assert_eq!(
            old_received.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        );
        drop(old);
        let event = key(A, true, Instant::now());
        tap.send(event);
        assert_eq!(received.try_recv(), Ok(event));
    }

    #[test]
    fn subscription_does_not_keep_driver_alive() {
        let tap = Arc::new(KeyTap::default());
        let published = Arc::downgrade(&tap);
        let (_guard, received) = tap.subscribe().unwrap();
        drop(tap);
        assert!(published.upgrade().is_none());
        assert_eq!(received.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }

    #[test]
    fn closed_receiver_is_discarded_without_affecting_ptt() {
        let tap = Arc::new(KeyTap::default());
        let (_guard, received) = tap.subscribe().unwrap();
        drop(received);
        let t0 = Instant::now();
        let mut feed = Feed::new(PttSettings::default(), t0);
        let event = key(RIGHT_CTRL, true, t0);
        tap.send(event);
        assert!(tap.subscriber.lock().unwrap().is_none());
        assert_eq!(feed.key(event), nothing());
        assert_eq!(feed.tick(at(t0, 250)), start(1));
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
    fn a_pending_press_keeps_polling_after_a_chord_until_the_release_is_seen() {
        let t0 = Instant::now();
        let mut feed = Feed::new(PttSettings::default(), t0);
        assert!(!feed.needs_poll());
        assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 0))), nothing());
        assert!(feed.needs_poll());
        assert_eq!(feed.tick(at(t0, 250)), start(1));
        assert_eq!(
            feed.key(key(A, true, at(t0, 300))),
            vec![PttEvent::Cancel { generation: 1 }]
        );
        // The session is over, but Right Ctrl's key-up was lost: polling goes on.
        assert!(!feed.active());
        assert!(feed.needs_poll());
        assert_eq!(feed.key(key(A, false, at(t0, 350))), nothing());
        assert_eq!(feed.poll(false, at(t0, 400)), nothing());
        assert!(!feed.needs_poll());
        // The next press works.
        assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 500))), nothing());
        assert_eq!(feed.tick(at(t0, 750)), start(2));
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

    fn toggle(t0: Instant) -> Feed {
        Feed::new(
            PttSettings {
                mode: Mode::Toggle,
                ..PttSettings::default()
            },
            t0,
        )
    }

    fn tap(feed: &mut Feed, t0: Instant, ms: u64) -> Vec<PttEvent> {
        assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, ms))), nothing());
        feed.key(key(RIGHT_CTRL, false, at(t0, ms + 100)))
    }

    #[test]
    fn toggle_taps_start_and_stop_without_watchdog_stopping_the_pause() {
        let t0 = Instant::now();
        let mut feed = toggle(t0);
        assert_eq!(tap(&mut feed, t0, 0), start(1));
        assert_eq!(feed.poll(false, at(t0, 500)), nothing());
        assert_eq!(feed.tick(at(t0, 1_000)), nothing());
        assert_eq!(tap(&mut feed, t0, 1_100), stop(1, StopReason::Toggled));
        assert_eq!(tap(&mut feed, t0, 1_500), start(2));
    }

    #[test]
    fn lost_short_release_does_not_toggle_a_latched_recording() {
        let t0 = Instant::now();
        let mut feed = toggle(t0);
        assert_eq!(tap(&mut feed, t0, 0), start(1));
        feed.key(key(RIGHT_CTRL, true, at(t0, 200)));
        assert_eq!(feed.poll(false, at(t0, 300)), nothing());
        assert!(feed.active());
        assert_eq!(tap(&mut feed, t0, 400), stop(1, StopReason::Toggled));
        // An old silence message must not discard the next pending gesture.
        feed.key(key(RIGHT_CTRL, true, at(t0, 600)));
        assert_eq!(feed.silence(1), nothing());
        assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 700))), start(2));
    }

    #[test]
    fn toggle_long_hold_and_watchdog_release() {
        for watchdog in [false, true] {
            let t0 = Instant::now();
            let mut feed = toggle(t0);
            assert_eq!(feed.key(key(RIGHT_CTRL, true, t0)), nothing());
            assert_eq!(feed.tick(at(t0, 399)), nothing());
            assert_eq!(feed.tick(at(t0, 400)), start(1));
            assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 500))), nothing());
            let out = if watchdog {
                feed.poll(false, at(t0, 600))
            } else {
                feed.key(key(RIGHT_CTRL, false, at(t0, 600)))
            };
            assert_eq!(
                out,
                stop(
                    1,
                    if watchdog {
                        StopReason::Watchdog
                    } else {
                        StopReason::Released
                    }
                )
            );
            assert!(!feed.active());
            assert_eq!(feed.tick(at(t0, 1_000)), nothing());
        }
    }

    #[test]
    fn toggle_tap_boundary_is_strict() {
        let t0 = Instant::now();
        let mut feed = toggle(t0);
        feed.key(key(RIGHT_CTRL, true, t0));
        assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 399))), start(1));
        feed.silence(1);
        feed.key(key(RIGHT_CTRL, true, at(t0, 500)));
        // No tick before release: a 400 ms press still cannot latch recording.
        assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 900))), nothing());
        assert!(!feed.active());
    }

    #[test]
    fn toggle_silence_is_generation_checked_and_does_not_reopen_held_key() {
        let t0 = Instant::now();
        let mut feed = toggle(t0);
        assert_eq!(tap(&mut feed, t0, 0), start(1));
        assert_eq!(feed.silence(1), stop(1, StopReason::Toggled));
        assert_eq!(feed.silence(1), nothing());
        assert_eq!(tap(&mut feed, t0, 200), start(2));
        assert_eq!(feed.silence(1), nothing());
        assert!(feed.active());
        assert_eq!(feed.silence(2), stop(2, StopReason::Toggled));
        feed.key(key(RIGHT_CTRL, true, at(t0, 400)));
        assert_eq!(feed.tick(at(t0, 800)), start(3));
        assert_eq!(feed.silence(3), stop(3, StopReason::Toggled));
        assert_eq!(feed.tick(at(t0, 1_000)), nothing());
        assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 1_100))), nothing());
    }

    #[test]
    fn toggle_chords_injection_and_lost_taps_do_not_start() {
        let t0 = Instant::now();
        let mut feed = toggle(t0);
        feed.key(injected(RIGHT_CTRL, true, t0));
        assert_eq!(feed.tick(at(t0, 500)), nothing());
        feed.key(key(RIGHT_CTRL, true, at(t0, 600)));
        assert_eq!(feed.poll(false, at(t0, 700)), nothing());
        assert_eq!(feed.tick(at(t0, 1_000)), nothing());
        feed.key(key(A, true, at(t0, 1_100)));
        assert_eq!(tap(&mut feed, t0, 1_200), nothing());
        feed.key(key(A, false, at(t0, 1_400)));
        assert_eq!(tap(&mut feed, t0, 1_500), start(1));
        feed.key(key(RIGHT_CTRL, true, at(t0, 1_700)));
        assert_eq!(
            feed.key(key(A, true, at(t0, 1_750))),
            vec![PttEvent::Cancel { generation: 1 }]
        );
        assert_eq!(feed.poll(false, at(t0, 1_800)), nothing());
    }

    #[test]
    fn toggle_deadline_is_thirty_seconds_even_without_speech() {
        let t0 = Instant::now();
        let mut feed = toggle(t0);
        assert_eq!(tap(&mut feed, t0, 0), start(1));
        assert_eq!(feed.tick(at(t0, 30_099)), nothing());
        assert_eq!(feed.tick(at(t0, 30_100)), stop(1, StopReason::TooLong));
        assert_eq!(feed.tick(at(t0, 31_000)), nothing());
        assert_eq!(tap(&mut feed, t0, 31_100), start(2));
    }

    #[test]
    fn mode_is_sampled_on_a_fresh_press_and_generations_remain_unique() {
        let t0 = Instant::now();
        let mut feed = toggle(t0);
        assert_eq!(tap(&mut feed, t0, 0), start(1));
        assert_eq!(
            feed.key_in_mode(key(RIGHT_CTRL, true, at(t0, 200)), Mode::Hold),
            stop(1, StopReason::Cancelled)
        );
        assert_eq!(feed.tick(at(t0, 450)), start(2));
        assert_eq!(
            feed.key_in_mode(key(RIGHT_CTRL, true, at(t0, 500)), Mode::Toggle),
            nothing()
        );
        assert_eq!(feed.silence(2), nothing());
        assert_eq!(
            feed.key(key(RIGHT_CTRL, false, at(t0, 600))),
            stop(2, StopReason::Released)
        );
        assert_eq!(
            feed.key_in_mode(key(RIGHT_CTRL, true, at(t0, 700)), Mode::Toggle),
            nothing()
        );
        assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 800))), start(3));
    }
}
