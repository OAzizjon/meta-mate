//! Pointing at the answer's target, in time with the speech: the WebView's timeline player
//! asks `point_at` when it reaches the sentence that points (src/mate.ts), so the beam
//! lights up while Mate says "here". The look the answer was given (screen.rs) resolves the
//! point; Mate flies beside the target without covering it (mm_core::placement) and the
//! highlight window shines on it (highlight.rs).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use mm_core::geom::PhysRect;
use mm_core::matescript::Point;
use mm_core::placement::place_mate;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::answer_stream::{AnswerMsg, AnswerStream};
use crate::flight::Flight;
use crate::screen::Look;
use mm_win32::winevent::{self, Follow};

/// Clearance between Mate and the target, in CSS pixels at 100 %.
const GAP: f64 = 24.0;

/// A SAM 3.1 outline asked for while the answer was still being written, by
/// (generation, pixel x, pixel y): pointing then waits for it instead of starting late.
type Outlines = HashMap<(u64, u32, u32), tauri::async_runtime::JoinHandle<Option<PhysRect>>>;

/// The look of the answer being played, by generation.
#[derive(Default)]
pub struct Pointing {
    current: Mutex<Option<(u64, Arc<Look>)>>,
    follower: Mutex<Option<Follower>>,
    outlines: Mutex<Outlines>,
}

impl Pointing {
    /// Starts SAM on pixel (x, y) of `generation`'s look now (voice.rs, while parsing).
    pub fn prefetch_outline(&self, app: &AppHandle, generation: u64, x: u32, y: u32) {
        let (Some(look), Some(proxy)) = (
            self.look(generation),
            app.try_state::<crate::proxy::Proxy>(),
        ) else {
            return;
        };
        let asked = self
            .outlines
            .lock()
            .is_ok_and(|outlines| outlines.contains_key(&(generation, x, y)));
        if asked {
            return;
        }
        let proxy = proxy.inner().clone();
        let task = tauri::async_runtime::spawn(async move {
            crate::sam::sam_outline(&proxy, &look, x, y).await
        });
        if let Ok(mut outlines) = self.outlines.lock() {
            // Only this answer's outlines are worth keeping (a dropped one is stopped: each
            // costs a SAM call); the same pixel is asked once.
            outlines.retain(|(owner, _, _), old| {
                let keep = *owner == generation;
                if !keep {
                    old.abort();
                }
                keep
            });
            if let Some(duplicate) = outlines.insert((generation, x, y), task) {
                duplicate.abort();
            }
        }
    }

    /// The outline asked for earlier, or a fresh one.
    async fn outline(
        &self,
        app: &AppHandle,
        generation: u64,
        look: &Look,
        x: u32,
        y: u32,
    ) -> Option<PhysRect> {
        let early = self.outlines.lock().ok()?.remove(&(generation, x, y));
        if let Some(task) = early {
            return task.await.ok().flatten();
        }
        let proxy = app.try_state::<crate::proxy::Proxy>()?.inner().clone();
        crate::sam::sam_outline(&proxy, look, x, y).await
    }

    pub fn set(&self, generation: u64, look: Look) {
        self.stop_following();
        if let Ok(mut current) = self.current.lock() {
            *current = Some((generation, Arc::new(look)));
        }
    }

    /// The teacher's current look (it changes every turn).
    pub fn set_shared(&self, generation: u64, look: Arc<Look>) {
        self.stop_following();
        if let Ok(mut current) = self.current.lock() {
            *current = Some((generation, look));
        }
    }

    pub fn look(&self, generation: u64) -> Option<Arc<Look>> {
        let current = self.current.lock().ok()?;
        let (pointed, look) = current.as_ref()?;
        (*pointed == generation).then(|| Arc::clone(look))
    }

    /// The generation that had a look, if any; the look is dropped (its screenshot too).
    pub fn take(&self) -> Option<u64> {
        self.stop_following();
        let mut current = self.current.lock().ok()?;
        current.take().map(|(generation, _)| generation)
    }

    pub fn generation(&self) -> Option<u64> {
        let current = self.current.lock().ok()?;
        current.as_ref().map(|(generation, _)| *generation)
    }

    fn stop_following(&self) {
        if let Ok(mut follower) = self.follower.lock() {
            follower.take();
        }
    }
}

/// A single shown target. Queued UI work is invalidated before joining the worker.
struct Follower {
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Follower {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Follower {
    fn start(
        app: &AppHandle,
        generation: u64,
        look: &Look,
        target: PhysRect,
        step: Option<(u32, u32)>,
    ) -> Option<Self> {
        let window = look.snapshot.target;
        let original = look.window_frame;
        let initial = mm_win32::window::foreground_target(&[])?;
        if initial.window != window
            || initial.frame != original
            || initial.region != look.snapshot.frame
        {
            return None;
        }
        let popups = popup_state(&initial);
        // A target on an unchanged popup must not be translated with its owner.
        let can_shift = !initial.popups.iter().any(|popup| {
            target.left < popup.right
                && popup.left < target.right
                && target.top < popup.bottom
                && popup.top < target.bottom
        });
        let (events, incoming) = mpsc::channel();
        let hook = winevent::start(window, events)
            .map_err(|error| {
                eprintln!("pointing: window hook: {error}");
            })
            .ok()?;
        let stopped = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&stopped);
        let app = app.clone();
        let thread = std::thread::Builder::new()
            .name("mm-pointing-follower".into())
            .spawn(move || {
                let _hook = hook;
                let mut last = original;
                while !cancelled.load(Ordering::Acquire) {
                    // Coalesce the event queue and query only the newest frame at ~30 Hz.
                    std::thread::sleep(Duration::from_millis(33));
                    let mut uncertain = false;
                    let mut moved = false;
                    for event in incoming.try_iter() {
                        match event.event {
                            // Minimized, scrolled: what the ring was on is gone or moved
                            // inside the window.
                            0x0016 | 0x0012 | 0x8015 => uncertain = true,
                            // The target itself destroyed or hidden.
                            0x8001 | 0x8003 => {
                                uncertain |= event.hwnd == window.handle && event.object == 0;
                            }
                            // The target window itself moved or resized: its frame and popups
                            // decide below. Children, tooltips, carets, hover, focus, names
                            // and values change all the time and never clear the ring.
                            winevent::LOCATION
                                if event.hwnd == window.handle
                                    && event.object == 0
                                    && event.child == 0 =>
                            {
                                moved = true;
                            }
                            _ => {}
                        }
                    }
                    if !uncertain && !moved {
                        continue;
                    }
                    let current = mm_win32::window::foreground_target(&[]);
                    let frame = current
                        .filter(|now| now.window == window && popup_state(now) == popups)
                        .map(|now| now.frame);
                    let change = if uncertain {
                        Follow::Clear
                    } else {
                        frame.map_or(Follow::Clear, |now| winevent::follow(last, now))
                    };
                    if change == Follow::Same {
                        continue;
                    }
                    let shifted = frame.and_then(|now| match winevent::follow(original, now) {
                        Follow::Same => Some(target),
                        Follow::Shift(dx, dy) => shifted_rect(target, dx, dy),
                        Follow::Clear => None,
                    });
                    let clear = change == Follow::Clear || !can_shift || shifted.is_none();
                    let active = Arc::clone(&cancelled);
                    let ui = app.clone();
                    let expected_popups = popups.clone();
                    if app
                        .run_on_main_thread(move || {
                            if active.load(Ordering::Acquire) {
                                return;
                            }
                            let Some(pointing) = ui.try_state::<Pointing>() else {
                                return;
                            };
                            if pointing.generation() != Some(generation) {
                                return;
                            }
                            // The frame may have changed again while waiting for the UI thread.
                            let valid =
                                mm_win32::window::foreground_target(&[]).is_some_and(|now| {
                                    now.window == window
                                        && Some(now.frame) == frame
                                        && popup_state(&now) == expected_popups
                                });
                            if clear || !valid {
                                stop_pointing(&ui);
                            } else if let Some(rect) = shifted
                                && render_target(&ui, generation, rect, step).is_none()
                            {
                                stop_pointing(&ui);
                            }
                        })
                        .is_err()
                        || clear
                    {
                        break;
                    }
                    if let Some(frame) = frame {
                        last = frame;
                    }
                }
            })
            .map_err(|error| eprintln!("pointing: follower thread: {error}"))
            .ok()?;
        Some(Self {
            stopped,
            thread: Some(thread),
        })
    }
}

fn shifted_rect(rect: PhysRect, dx: i32, dy: i32) -> Option<PhysRect> {
    PhysRect::new(
        rect.left.checked_add(dx)?,
        rect.top.checked_add(dy)?,
        rect.right.checked_add(dx)?,
        rect.bottom.checked_add(dy)?,
    )
    .ok()
}

fn popup_state(target: &mm_win32::window::Target) -> Vec<(u64, PhysRect)> {
    let mut popups: Vec<_> = target
        .popup_hwnds
        .iter()
        .zip(&target.popups)
        .map(|(hwnd, rect)| (hwnd.0 as usize as u64, *rect))
        .collect();
    popups.sort_by_key(|(handle, _)| *handle);
    popups
}

/// `step` of a `<step>` tag, as the window sends it.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct StepNumber {
    pub number: u32,
    pub total: u32,
}

/// The target in Mate's window, CSS pixels: where his own eyes and beam turn.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct MateRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// Shines on the point of generation `generation` and moves Mate beside it. None when it
/// cannot be shown: no look for that answer, nothing pointable there (invariant 5), or the
/// target on a monitor the highlight does not cover (Mate says so).
#[tauri::command]
pub async fn point_at(
    app: AppHandle,
    generation: u64,
    point: Point,
    step: Option<StepNumber>,
) -> Option<MateRect> {
    let pointing = app.try_state::<Pointing>()?;
    let look = pointing.look(generation)?;
    // The picture must still be true: the window there, unmoved.
    if !mm_win32::window::still_shown(look.snapshot.target, look.window_frame) {
        stop_pointing(&app);
        app.state::<AnswerStream>().send(AnswerMsg::Line {
            generation,
            key: "robot.window_closed",
        });
        return None;
    }
    let resolved = match (look.resolve(&point), &point) {
        (Some(target), _) => Some(target),
        // A pixel on nothing UI Automation lists (an app without a tree): the outline of
        // what is there, from SAM 3.1, or nothing (sam.rs).
        (None, Point::Pixel(x, y)) if crate::sam::outline_allowed(&look, *x, *y) => {
            let outline = pointing.outline(&app, generation, &look, *x, *y).await;
            if outline.is_none() && pointing.generation() == Some(generation) {
                // The sentence was let through for SAM; it found nothing: say so.
                app.state::<AnswerStream>().send(AnswerMsg::Line {
                    generation,
                    key: "robot.cant_find",
                });
            }
            outline
        }
        (None, _) => None,
    };
    // A newer press may have ended this answer while SAM worked: its ring is not ours to
    // clear, and nothing of ours may be shown.
    if pointing.generation() != Some(generation) {
        return None;
    }
    // The window may have moved or closed during the wait: the picture must still be true.
    if !mm_win32::window::still_shown(look.snapshot.target, look.window_frame) {
        stop_pointing(&app);
        return None;
    }
    let Some(target) = resolved else {
        // Nothing pointable there: the ring of an earlier point must not stay lit.
        stop_pointing(&app);
        return None;
    };
    show_target(&app, generation, target, step.map(|s| (s.number, s.total)))
}

/// Mate beside `target` and the highlight on it (also the teacher's way of pointing).
/// None when it cannot be shown: on a monitor the highlight does not cover (Mate says so)
/// or nowhere Mate fits.
pub fn show_target(
    app: &AppHandle,
    generation: u64,
    target: PhysRect,
    step: Option<(u32, u32)>,
) -> Option<MateRect> {
    let pointing = app.try_state::<Pointing>()?;
    pointing.stop_following();
    let look = pointing.look(generation)?;
    // Following a moved window is a nicety: without it the ring still shows, and the
    // checks before every point (still_shown) keep it honest.
    let follower = Follower::start(app, generation, &look, target, step);
    if pointing.generation() != Some(generation) {
        return None;
    }
    let shown = render_target(app, generation, target, step);
    if shown.is_none() {
        stop_pointing(app);
        return None;
    }
    let mut following = pointing.follower.lock().ok()?;
    // A new answer may have replaced the look while the hook was being installed.
    if pointing.generation() != Some(generation) || !Arc::ptr_eq(&look, &pointing.look(generation)?)
    {
        let _ = crate::highlight::clear(app);
        return None;
    }
    *following = follower;
    shown
}

fn render_target(
    app: &AppHandle,
    generation: u64,
    target: PhysRect,
    step: Option<(u32, u32)>,
) -> Option<MateRect> {
    // Any monitor: each has its highlight window, by index (overlay.rs).
    let monitors = mm_win32::monitors::monitors();
    let (index, monitor) = monitors
        .iter()
        .enumerate()
        .find(|(_, m)| m.area.contains(target.center()))?;
    let scale = monitor.scale();
    let size = (crate::overlay::MATE_SIZE * scale).round() as i32;
    let gap = (GAP * scale).round() as i32;
    let placement = place_mate(target, monitor.work_area, size, size, gap)?;
    #[cfg(debug_assertions)]
    eprintln!(
        "pointing: generation {generation} target {target:?} mate {:?} ({:?})",
        placement.mate, placement.side
    );
    #[cfg(not(debug_assertions))]
    let _ = generation; // only the log above uses it
    if let Some(flight) = app.try_state::<Flight>() {
        flight.fly_to(placement.mate);
    }
    let shown = crate::highlight::show(app, index, monitor, target, Some(placement.mate), step);
    if let Err(error) = shown {
        eprintln!("pointing: highlight: {error:?}");
        return None;
    }
    Some(in_mate(target, placement.mate, scale))
}

/// The target relative to Mate's window at `mate`, in that window's CSS pixels.
fn in_mate(target: PhysRect, mate: PhysRect, scale: f64) -> MateRect {
    let css = |phys: i32, origin: i32| f64::from(phys - origin) / scale;
    MateRect {
        left: css(target.left, mate.left),
        top: css(target.top, mate.top),
        right: css(target.right, mate.left),
        bottom: css(target.bottom, mate.top),
    }
}

/// Ends pointing: the highlight goes out and Mate flies home.
pub fn stop_pointing(app: &AppHandle) {
    if let Some(pointing) = app.try_state::<Pointing>() {
        pointing.take();
    }
    let _ = crate::highlight::clear(app);
    let home = crate::overlay::home(&mm_win32::monitors::monitors());
    if let (Some(flight), Some(home)) = (app.try_state::<Flight>(), home) {
        flight.land(home);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_is_given_in_mates_own_css_pixels() {
        let target = PhysRect::new(1000, 600, 1150, 660).unwrap();
        let mate = PhysRect::new(1186, 520, 1516, 850).unwrap();
        let rect = in_mate(target, mate, 1.5);
        assert!((rect.left - (-124.0)).abs() < 1e-9);
        assert!((rect.top - (53.0 + 1.0 / 3.0)).abs() < 1e-9);
        assert!((rect.right - (-24.0)).abs() < 1e-9);
        assert!((rect.bottom - 93.333_333_333_333_33).abs() < 1e-6);
    }

    #[test]
    fn translation_preserves_target_size_and_refuses_overflow() {
        let target = PhysRect::new(-100, 20, 0, 60).unwrap();
        assert_eq!(
            shifted_rect(target, 25, -30),
            PhysRect::new(-75, -10, 25, 30).ok()
        );
        assert_eq!(shifted_rect(target, i32::MIN, 0), None);
        assert_eq!(shifted_rect(target, 0, i32::MAX), None);
    }
}
