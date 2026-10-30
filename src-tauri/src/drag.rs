//! Native cursor polling: dragging keeps working outside the SVG and WebView, with
//! every placement on the UI thread through SWP_NOACTIVATE. One frame is queued at a
//! time; no lock is held across SetWindowPos or while waiting for the UI thread.

use std::time::Duration;

use mm_core::drag::{ClickGesture, Drag};
use mm_core::geom::PhysPoint;
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::flight::Flight;
use crate::pointing::Pointing;

/// Only the robot's own left-button pointerdown starts a drag (src/mate.ts).
#[tauri::command]
pub async fn mate_drag_start(app: AppHandle, window: WebviewWindow, x: f64, y: f64) {
    if window.label() != "mate" || !x.is_finite() || !y.is_finite() {
        return;
    }
    let (Ok(position), Ok(scale)) = (window.inner_position(), window.scale_factor()) else {
        return;
    };
    // Preserve pointerdown even when a quick release/move precedes IPC delivery.
    // WebView client coordinates are CSS pixels; native polling uses physical pixels.
    let cursor = PhysPoint {
        x: position.x.saturating_add((x * scale).round() as i32),
        y: position.y.saturating_add((y * scale).round() as i32),
    };
    let Some(flight) = app.try_state::<Flight>() else {
        return;
    };
    let start = if app.state::<Pointing>().generation().is_none() {
        flight.begin_drag()
    } else {
        None
    };
    let Some(start) = start else {
        // Pointing and flight still forbid dragging, but a click can interrupt speech.
        let mut gesture = ClickGesture::new(cursor);
        while let Some((cursor, held)) = mm_win32::mouse::drag_pointer() {
            if !gesture.update(cursor) {
                break;
            }
            if !held {
                if !crate::explain::clicked(&app) {
                    crate::ptt::click();
                }
                break;
            }
            tokio::time::sleep(Duration::from_millis(16)).await;
        }
        return;
    };
    let drag = Drag::new(start, cursor);
    let mut gesture = ClickGesture::new(cursor);
    let monitors = std::sync::Arc::new(mm_win32::monitors::monitors());
    loop {
        let monitors = monitors.clone();
        let ui = app.clone();
        let (sent, received) = tokio::sync::oneshot::channel();
        if app
            .run_on_main_thread(move || {
                let keep_going = frame(&ui, drag, &mut gesture, &monitors);
                let _ = sent.send((keep_going, gesture));
            })
            .is_err()
        {
            break;
        }
        match received.await {
            Ok((true, next)) => gesture = next,
            _ => break,
        }
        tokio::time::sleep(Duration::from_millis(16)).await;
    }
    flight.end_drag();
}

fn frame(
    app: &AppHandle,
    drag: Drag,
    gesture: &mut ClickGesture,
    monitors: &[mm_win32::monitors::Monitor],
) -> bool {
    let flight = app.state::<Flight>();
    // A pending flight or a new look takes priority. Its worker waits until end_drag.
    if !flight.drag_allowed() || app.state::<Pointing>().generation().is_some() {
        return false;
    }
    let Some((cursor, held)) = mm_win32::mouse::drag_pointer() else {
        return false;
    };
    if gesture.update(cursor) && !held {
        // While a "What's this?" offer is open the click asks it; otherwise it talks.
        if !crate::explain::clicked(app) {
            crate::ptt::click();
        }
        return false;
    }
    // Nearest work area handles taskbars and gaps between connected monitors too.
    let Some(monitor) = monitors.iter().min_by(|a, b| {
        a.work_area
            .distance_to(cursor)
            .total_cmp(&b.work_area.distance_to(cursor))
    }) else {
        return false;
    };
    let Some(rect) = drag.at(cursor, monitor.work_area) else {
        return false;
    };
    let Some(window) = app.get_webview_window("mate") else {
        return false;
    };
    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    let dpi = mm_win32::dpi::dpi_for_window(hwnd);
    if mm_win32::overlay::place(hwnd, rect).is_err() {
        return false;
    }
    // A monitor scale transition can resize the window; enforce physical bounds again.
    if mm_win32::dpi::dpi_for_window(hwnd) != dpi && mm_win32::overlay::place(hwnd, rect).is_err() {
        return false;
    }
    flight.dragged(rect);
    if !held {
        flight.set_home(rect);
        crate::settings::save_mate_home(app, rect);
        // Let go on a control: offer to explain it (explain.rs). Nothing leaves the PC.
        crate::explain::dropped(app, cursor);
    }
    held
}
