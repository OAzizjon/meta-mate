//! Mate's flight: one worker thread for the app's lifetime moves Mate's window along
//! `mm_core::flight::flight_path` at ~60 fps, every frame through `overlay::place`, which
//! never activates it (invariant 1). A newer request replaces the flight in progress at
//! once and starts from the last frame placed, even mid-flight.
//!
//! The lock is never held while a frame is placed: SetWindowPos on a window of another
//! thread waits for that (UI) thread, which may be asking for the next flight meanwhile.

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mm_core::flight::flight_path;
use mm_core::geom::PhysRect;
use mm_win32::HWND;
use mm_win32::overlay;

/// ~60 fps.
const FRAME_MS: u32 = 16;
/// A flight lasts this long plus `MS_PER_PX` per pixel flown; `flight_path` clamps it.
const BASE_MS: f64 = 250.0;
const MS_PER_PX: f64 = 0.6;

/// Moves Mate's window; one per app, kept in Tauri state.
pub struct Flight {
    shared: Arc<Shared>,
}

/// The schedule and its two wake-ups, shared by `Flight` and its worker.
struct Shared {
    schedule: Mutex<Schedule>,
    /// A new request, or the `Flight` dropped: wakes the worker.
    wake: Condvar,
    /// The worker ran out of requests: wakes `wait_idle`.
    idle: Condvar,
}

/// What the callers and the worker agree on. Pure, so the scheduling is tested without a
/// window or a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Schedule {
    home: PhysRect,
    dragging: bool,
    /// The last rectangle placed (`start` before the first frame).
    position: PhysRect,
    /// The newest request the worker has not taken yet: it replaces an older one and ends
    /// the flight in progress.
    pending: Option<PhysRect>,
    /// The worker is flying.
    flying: bool,
    /// The `Flight` is gone, or its worker never started.
    closed: bool,
}

impl Schedule {
    fn new(position: PhysRect) -> Self {
        Self {
            home: position,
            dragging: false,
            position,
            pending: None,
            flying: false,
            closed: false,
        }
    }

    /// A newer request replaces whatever came before; an empty rectangle is dropped
    /// (public rectangle fields allow struct literals).
    fn request(&mut self, to: PhysRect) {
        if !self.closed && PhysRect::new(to.left, to.top, to.right, to.bottom).is_ok() {
            self.pending = Some(to);
        }
    }

    /// The worker takes the newest request: from where Mate is now (the last frame
    /// placed, even mid-flight) to it.
    fn take(&mut self) -> Option<(PhysRect, PhysRect)> {
        if self.dragging {
            return None;
        }
        let to = self.pending.take();
        self.flying = to.is_some();
        to.map(|to| (self.position, to))
    }

    /// A frame was placed: Mate is there now. False when the flight must end.
    fn placed(&mut self, rect: PhysRect) -> bool {
        self.position = rect;
        self.carry_on()
    }

    /// No newer request, and the `Flight` still there.
    fn carry_on(&self) -> bool {
        self.pending.is_none() && !self.closed
    }

    #[allow(dead_code)] // used by wait_idle
    fn idle(&self) -> bool {
        self.pending.is_none() && !self.flying && !self.dragging
    }
}

impl Flight {
    /// Claims the idle window before the first drag frame. Pending flights count as busy.
    pub fn begin_drag(&self) -> Option<PhysRect> {
        let mut schedule = lock(&self.shared.schedule);
        if schedule.closed || schedule.dragging || !schedule.idle() {
            return None;
        }
        schedule.dragging = true;
        Some(schedule.position)
    }

    pub fn drag_allowed(&self) -> bool {
        let schedule = lock(&self.shared.schedule);
        schedule.dragging && schedule.pending.is_none() && !schedule.closed
    }

    /// Called only after a successful no-activate placement.
    pub fn dragged(&self, rect: PhysRect) {
        lock(&self.shared.schedule).position = rect;
    }

    pub fn end_drag(&self) {
        lock(&self.shared.schedule).dragging = false;
        self.shared.wake.notify_one();
    }

    pub fn set_home(&self, home: PhysRect) {
        lock(&self.shared.schedule).home = home;
    }

    pub fn home(&self) -> PhysRect {
        lock(&self.shared.schedule).home
    }

    /// Flies `mate` (Mate's window, now at `start`) from a worker thread that lives as long
    /// as the app.
    pub fn new(mate: HWND, start: PhysRect) -> Self {
        // The raw handle crosses the thread as a number; SetWindowPos on it is thread-safe.
        let raw = mate.0 as isize;
        Self::with_sink(start, move |rect| {
            let mate = HWND(raw as *mut core::ffi::c_void);
            overlay::place(mate, rect).is_ok()
        })
    }

    /// `place` puts one frame on the screen and says whether it did: Mate's window in the
    /// app, a recorder in tests.
    fn with_sink(
        start: PhysRect,
        mut place: impl FnMut(PhysRect) -> bool + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            schedule: Mutex::new(Schedule::new(start)),
            wake: Condvar::new(),
            idle: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("mm-flight".into())
            .spawn(move || run(&worker, &mut place));
        if let Err(error) = spawned {
            // No worker: Mate stays where he is and every request is dropped.
            eprintln!("flight: {error}");
            lock(&shared.schedule).closed = true;
        }
        Self { shared }
    }

    /// Flies from where Mate is now (the last frame placed, even mid-flight) to `to`,
    /// replacing any flight in progress at once.
    pub fn fly_to(&self, to: PhysRect) {
        lock(&self.shared.schedule).request(to);
        self.shared.wake.notify_one();
    }

    /// Flies back to `home`, like `fly_to`.
    pub fn land(&self, home: PhysRect) {
        self.fly_to(home);
    }

    /// The last rectangle placed.
    pub fn position(&self) -> PhysRect {
        lock(&self.shared.schedule).position
    }

    /// For the teacher (Day 5), which captures right after Mate moves.
    #[allow(dead_code)]
    /// Waits up to `timeout` for Mate to stop moving; true when he has. Never call it on
    /// the thread that owns Mate's window: every frame's SetWindowPos waits for that
    /// thread, so the flight would stall until the timeout.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let schedule = lock(&self.shared.schedule);
        let (schedule, _) = self
            .shared
            .idle
            .wait_timeout_while(schedule, timeout, |schedule| !schedule.idle())
            .unwrap_or_else(PoisonError::into_inner);
        schedule.idle()
    }
}

impl Drop for Flight {
    /// Ends the worker at its next wake-up. Never joined: it may be inside SetWindowPos,
    /// waiting for the UI thread that drops us.
    fn drop(&mut self) {
        lock(&self.shared.schedule).closed = true;
        self.shared.wake.notify_one();
    }
}

impl fmt::Debug for Flight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Flight")
            .field("position", &self.position())
            .finish_non_exhaustive()
    }
}

/// A panic elsewhere never stops Mate: a poisoned lock is taken over as it is.
fn lock(schedule: &Mutex<Schedule>) -> MutexGuard<'_, Schedule> {
    schedule.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The worker: one flight at a time, for as long as the `Flight` lives.
fn run(shared: &Shared, place: &mut impl FnMut(PhysRect) -> bool) {
    while let Some((from, to)) = next_job(shared) {
        let frames = flight_path(from, to, duration_ms(from, to), FRAME_MS);
        fly(shared, &frames, place);
    }
}

/// Waits for the next request; None once the `Flight` is gone.
fn next_job(shared: &Shared) -> Option<(PhysRect, PhysRect)> {
    let mut schedule = lock(&shared.schedule);
    loop {
        if schedule.closed {
            return None;
        }
        if let Some(job) = schedule.take() {
            return Some(job);
        }
        shared.idle.notify_all();
        schedule = shared
            .wake
            .wait(schedule)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

/// Places `frames` on the clock until the last one, a newer request, or a place error
/// (which ends the flight quietly: Mate stays at the last frame placed).
fn fly(shared: &Shared, frames: &[PhysRect], place: &mut impl FnMut(PhysRect) -> bool) {
    let started = Instant::now();
    let mut index = 0;
    while let Some(&frame) = frames.get(index) {
        if !place(frame) {
            return;
        }
        let mut schedule = lock(&shared.schedule);
        if !schedule.placed(frame) || index + 1 >= frames.len() {
            return;
        }
        // Until the next frame is due, unless a newer request comes first.
        let wait = frame_time(index + 1).saturating_sub(started.elapsed());
        let (schedule, _) = shared
            .wake
            .wait_timeout_while(schedule, wait, |schedule| schedule.carry_on())
            .unwrap_or_else(PoisonError::into_inner);
        if !schedule.carry_on() {
            return;
        }
        drop(schedule);
        index = next_frame(index, started.elapsed(), frames.len());
    }
}

/// About 250 ms plus 0.6 ms per pixel between the two centers (Mate's size changes
/// between monitors of different scale); `flight_path` clamps it.
fn duration_ms(from: PhysRect, to: PhysRect) -> u32 {
    let (a, b) = (from.center(), to.center());
    let dx = f64::from(b.x) - f64::from(a.x);
    let dy = f64::from(b.y) - f64::from(a.y);
    (BASE_MS + MS_PER_PX * dx.hypot(dy)).round() as u32
}

/// When frame `index` is due, from the flight's start (the first one at once).
fn frame_time(index: usize) -> Duration {
    Duration::from_millis(u64::from(FRAME_MS).saturating_mul(index as u64))
}

/// The frame to place after frame `placed`, `elapsed` after the flight started: the next
/// one, or the one due by now when placing fell behind (the flight keeps its duration
/// rather than slowing down), never past the last one (exactly `to`).
fn next_frame(placed: usize, elapsed: Duration, count: usize) -> usize {
    let due = elapsed.as_millis() / u128::from(FRAME_MS);
    let due = usize::try_from(due).unwrap_or(usize::MAX);
    (placed + 1).max(due).min(count.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError};

    use super::*;

    /// Longer than any flight (`flight_path` clamps to 900 ms).
    const WAIT: Duration = Duration::from_secs(5);

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PhysRect {
        PhysRect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Mate's corner of a 1920x1080 monitor at 100 %.
    fn home() -> PhysRect {
        rect(1676, 836, 1896, 1056)
    }

    /// Mate beside a button in the middle of the screen.
    fn beside() -> PhysRect {
        rect(700, 300, 920, 520)
    }

    /// A flight whose frames go to a channel instead of a window.
    fn recorded(start: PhysRect) -> (Flight, Receiver<PhysRect>) {
        let (frames, placed) = mpsc::channel();
        let flight = Flight::with_sink(start, move |rect| frames.send(rect).is_ok());
        (flight, placed)
    }

    #[test]
    fn duration_grows_with_the_distance_between_centers() {
        let mate = rect(0, 0, 200, 200);
        assert_eq!(duration_ms(mate, mate), 250);
        // Same center, another size (a monitor of another scale): no distance.
        assert_eq!(duration_ms(mate, rect(25, 25, 175, 175)), 250);
        // 300 across and 400 down: 500 px.
        assert_eq!(duration_ms(mate, rect(300, 400, 500, 600)), 550);
        assert_eq!(duration_ms(mate, rect(1000, 0, 1200, 200)), 850);
        assert_eq!(duration_ms(rect(1000, 0, 1200, 200), mate), 850);
    }

    #[test]
    fn a_newer_request_replaces_the_flight_and_starts_from_the_last_frame() {
        let mut schedule = Schedule::new(home());
        assert!(schedule.idle());
        assert_eq!(schedule.take(), None);
        schedule.request(beside());
        assert!(!schedule.idle());
        assert_eq!(schedule.take(), Some((home(), beside())));
        assert!(!schedule.idle());
        let first = rect(1600, 790, 1820, 1010);
        assert!(schedule.placed(first));
        // Landing mid-flight: the frame being placed is the last one of this flight.
        schedule.request(home());
        let second = rect(1500, 730, 1720, 950);
        assert!(!schedule.placed(second));
        assert_eq!(schedule.position, second);
        assert_eq!(schedule.take(), Some((second, home())));
        assert!(schedule.placed(home()));
        assert_eq!(schedule.take(), None);
        assert!(schedule.idle());
    }

    #[test]
    fn only_the_newest_waiting_request_flies() {
        let mut schedule = Schedule::new(home());
        schedule.request(beside());
        schedule.request(rect(100, 100, 320, 320));
        assert_eq!(schedule.take(), Some((home(), rect(100, 100, 320, 320))));
        assert_eq!(schedule.take(), None);
    }

    #[test]
    fn dragging_reserves_the_idle_window_and_a_flight_waits_for_release() {
        let (flight, placed) = recorded(home());
        assert_eq!(flight.begin_drag(), Some(home()));
        assert_eq!(flight.begin_drag(), None);
        assert!(!flight.wait_idle(Duration::ZERO));
        let moved = rect(100, 100, 320, 320);
        flight.dragged(moved);
        flight.set_home(moved);
        assert_eq!(flight.home(), moved);
        flight.fly_to(beside());
        assert!(!flight.drag_allowed());
        // Taking a pending request must not let the flight worker enter SetWindowPos.
        assert_eq!(lock(&flight.shared.schedule).take(), None);
        assert_eq!(flight.position(), moved);
        assert!(placed.try_recv().is_err());
        flight.end_drag();
        assert!(flight.wait_idle(WAIT));
        assert_eq!(flight.position(), beside());
        assert_eq!(flight.home(), moved);
    }

    #[test]
    fn pending_or_active_flights_cannot_be_dragged() {
        // No worker: inspect both scheduling states without racing its next_job.
        let flight = Flight {
            shared: Arc::new(Shared {
                schedule: Mutex::new(Schedule::new(home())),
                wake: Condvar::new(),
                idle: Condvar::new(),
            }),
        };
        let mut schedule = lock(&flight.shared.schedule);
        schedule.request(beside());
        drop(schedule);
        assert_eq!(flight.begin_drag(), None);
        let mut schedule = lock(&flight.shared.schedule);
        assert!(schedule.take().is_some());
        drop(schedule);
        assert_eq!(flight.begin_drag(), None);
    }

    #[test]
    fn empty_rectangles_and_requests_after_closing_are_dropped() {
        let mut schedule = Schedule::new(home());
        schedule.request(rect(700, 300, 700, 520));
        schedule.request(rect(700, 520, 920, 300));
        assert!(schedule.idle());
        schedule.request(beside());
        assert!(schedule.take().is_some());
        schedule.closed = true;
        assert!(!schedule.placed(rect(1600, 790, 1820, 1010)));
        schedule.request(home());
        assert_eq!(schedule.pending, None);
    }

    #[test]
    fn frames_keep_the_clock_and_never_skip_the_last() {
        let ms = Duration::from_millis;
        assert_eq!(frame_time(0), ms(0));
        assert_eq!(frame_time(3), ms(48));
        // On time, a little late, then behind: the frame due now (3 x 16 = 48 ms).
        assert_eq!(next_frame(0, ms(16), 10), 1);
        assert_eq!(next_frame(0, ms(30), 10), 1);
        assert_eq!(next_frame(0, ms(50), 10), 3);
        // Woken early: still forward.
        assert_eq!(next_frame(4, ms(0), 10), 5);
        // Far behind: straight to the last frame, never past it.
        assert_eq!(next_frame(2, ms(10_000), 10), 9);
        assert_eq!(next_frame(8, ms(10_000), 10), 9);
    }

    #[test]
    fn flies_frame_by_frame_and_lands_exactly() {
        let (flight, placed) = recorded(home());
        assert!(flight.wait_idle(Duration::ZERO));
        flight.fly_to(beside());
        assert!(flight.wait_idle(WAIT));
        assert_eq!(flight.position(), beside());
        let frames: Vec<PhysRect> = placed.try_iter().collect();
        // Frames may be skipped when placing falls behind, never invented.
        let path = flight_path(home(), beside(), duration_ms(home(), beside()), FRAME_MS);
        assert!(frames.iter().all(|frame| path.contains(frame)));
        assert_eq!(frames.last(), Some(&beside()));

        flight.land(home());
        assert!(flight.wait_idle(WAIT));
        assert_eq!(flight.position(), home());
        assert_eq!(placed.try_iter().last(), Some(home()));
    }

    #[test]
    fn a_newer_request_replaces_the_flight_in_progress_at_once() {
        let (frames, placed) = mpsc::channel();
        let (entered, first_frame) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let mut count = 0;
        let flight = Flight::with_sink(home(), move |rect| {
            count += 1;
            // The first frame waits until the test has asked for the next flight.
            if count == 1 {
                let _ = entered.send(());
                let _ = released.recv();
            }
            frames.send(rect).is_ok()
        });
        flight.fly_to(rect(100, 100, 320, 320));
        first_frame.recv_timeout(WAIT).unwrap();
        // The first frame is still being placed: Mate has not moved yet.
        assert_eq!(flight.position(), home());
        flight.land(home());
        assert!(!flight.wait_idle(Duration::ZERO));
        release.send(()).unwrap();
        assert!(flight.wait_idle(WAIT));
        assert_eq!(flight.position(), home());
        // One frame of the first flight, then the flight home from there.
        let frames: Vec<PhysRect> = placed.try_iter().collect();
        let first = frames[0];
        let back = flight_path(first, home(), duration_ms(first, home()), FRAME_MS);
        assert!(frames[1..].iter().all(|frame| back.contains(frame)));
        assert_eq!(frames.last(), Some(&home()));
    }

    #[test]
    fn a_place_error_ends_that_flight_quietly() {
        let broken = Arc::new(AtomicBool::new(true));
        let flight = Flight::with_sink(home(), {
            let broken = Arc::clone(&broken);
            move |_: PhysRect| !broken.load(Ordering::SeqCst)
        });
        flight.fly_to(beside());
        assert!(flight.wait_idle(WAIT));
        assert_eq!(flight.position(), home());
        // The worker is still there for the next flight.
        broken.store(false, Ordering::SeqCst);
        flight.fly_to(beside());
        assert!(flight.wait_idle(WAIT));
        assert_eq!(flight.position(), beside());
    }

    #[test]
    fn dropping_the_flight_ends_its_worker() {
        let (flight, placed) = recorded(home());
        drop(flight);
        // The worker dropped its sink, and with it the channel's only sender.
        assert_eq!(
            placed.recv_timeout(WAIT),
            Err(RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn debug_shows_the_position_only() {
        let (flight, _placed) = recorded(rect(0, 0, 220, 220));
        assert_eq!(
            format!("{flight:?}"),
            "Flight { position: PhysRect { left: 0, top: 0, right: 220, bottom: 220 }, .. }"
        );
    }

    #[test]
    fn can_live_in_tauri_state() {
        fn send_sync<T: Send + Sync + 'static>() {}
        send_sync::<Flight>();
    }
}
