//! Pointing at the answer's target, in time with the speech: the WebView's timeline player
//! asks `point_at` when it reaches the sentence that points (src/mate.ts), so the beam
//! lights up while Mate says "here". The look the answer was given (screen.rs) resolves the
//! point; Mate flies beside the target without covering it (mm_core::placement) and the
//! highlight window shines on it (highlight.rs).

use std::sync::{Arc, Mutex};

use mm_core::geom::PhysRect;
use mm_core::matescript::Point;
use mm_core::placement::place_mate;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::answer_stream::{AnswerMsg, AnswerStream};
use crate::flight::Flight;
use crate::screen::Look;

/// Clearance between Mate and the target, in CSS pixels at 100 %.
const GAP: f64 = 24.0;

/// The look of the answer being played, by generation.
#[derive(Default)]
pub struct Pointing {
    current: Mutex<Option<(u64, Arc<Look>)>>,
}

impl Pointing {
    pub fn set(&self, generation: u64, look: Look) {
        if let Ok(mut current) = self.current.lock() {
            *current = Some((generation, Arc::new(look)));
        }
    }

    /// The teacher's current look (it changes every turn).
    pub fn set_shared(&self, generation: u64, look: Arc<Look>) {
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
        let mut current = self.current.lock().ok()?;
        current.take().map(|(generation, _)| generation)
    }

    pub fn generation(&self) -> Option<u64> {
        let current = self.current.lock().ok()?;
        current.as_ref().map(|(generation, _)| *generation)
    }
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
pub fn point_at(
    app: AppHandle,
    generation: u64,
    point: Point,
    step: Option<StepNumber>,
) -> Option<MateRect> {
    let pointing = app.try_state::<Pointing>()?;
    let look = pointing.look(generation)?;
    // The picture must still be true: the window there, unmoved.
    if !mm_win32::window::still_shown(look.snapshot.target, look.window_frame) {
        let _ = crate::highlight::clear(&app);
        app.state::<AnswerStream>().send(AnswerMsg::Line {
            generation,
            key: "robot.window_closed",
        });
        return None;
    }
    let Some(target) = look.resolve(&point) else {
        // Nothing pointable there: the ring of an earlier point must not stay lit.
        let _ = crate::highlight::clear(&app);
        return None;
    };
    // A newer press may have ended this answer while we resolved: show nothing then.
    if pointing.generation() != Some(generation) {
        return None;
    }
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
}
