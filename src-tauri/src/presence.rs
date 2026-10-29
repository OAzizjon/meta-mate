//! Mate's page menu and session visibility. Window changes run on the event thread.

use std::sync::Mutex;
#[cfg(windows)]
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
pub struct Presence(
    pub Mutex<mm_core::presence::Presence>,
    Mutex<Option<mm_core::geom::PhysRect>>,
);

/// Mate's window handle while he is hidden, else 0: the capture (screen.rs) has no app
/// handle and must not show him again.
#[cfg(windows)]
static HIDDEN: AtomicIsize = AtomicIsize::new(0);

/// True for Mate's window while he is hidden.
#[cfg(windows)]
pub fn is_hidden(hwnd: isize) -> bool {
    hwnd != 0 && HIDDEN.load(Ordering::Acquire) == hwnd
}

#[tauri::command]
pub fn quit_app(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    if !matches!(window.label(), "home" | "mate") {
        return Err("not a Home or Mate window".into());
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn hide_mate(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "mate" {
        return Err("not the Mate window".into());
    }
    hide(&app);
    Ok(())
}

pub fn hide(app: &AppHandle) {
    // Start the hour at the action, including the explanation's three seconds.
    let started = Instant::now();
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window("mate") else {
            return;
        };
        handle.state::<Presence>().0.lock().unwrap().hide(started);
        crate::show_tray_wake(&handle, true);
        let _ = window.emit("mate-hiding", serde_json::json!({ "minutes": 60 }));
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let delayed = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                if delayed
                    .state::<Presence>()
                    .0
                    .lock()
                    .unwrap()
                    .hide_is_current(started)
                    && let Some(window) = delayed.get_webview_window("mate")
                {
                    // The raw hide, like the raw show in `wake`: Tauri's own setters may
                    // rewrite the no-activate styles (invariant 1).
                    #[cfg(windows)]
                    if let Ok(hwnd) = window.hwnd() {
                        HIDDEN.store(hwnd.0 as isize, Ordering::Release);
                        mm_win32::overlay::hide(hwnd);
                    }
                    #[cfg(not(windows))]
                    let _ = window.hide();
                }
            });
            tokio::time::sleep_until((started + mm_core::presence::HIDE_DURATION).into()).await;
            wake(&handle, true);
        });
    });
}

pub fn wake(app: &AppHandle, timer: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let show = {
            let state = handle.state::<Presence>();
            let mut state = state.0.lock().unwrap();
            if timer {
                state.wake_if_due(Instant::now())
            } else {
                state.wake()
            }
        };
        if !show {
            return;
        }
        #[cfg(windows)]
        HIDDEN.store(0, Ordering::Release);
        crate::show_tray_wake(&handle, false);
        if let Some(window) = handle.get_webview_window("mate") {
            let _ = window.emit("mate-woke", ());
            #[cfg(windows)]
            if let Ok(hwnd) = window.hwnd() {
                rehome(&handle, hwnd);
                mm_win32::overlay::show(hwnd);
            }
            #[cfg(not(windows))]
            let _ = window.show();
        }
    });
}

/// A monitor unplugged while Mate slept would wake him where nobody sees him: he goes to
/// his starting home instead (the saved one when its monitor is still there).
#[cfg(windows)]
fn rehome(app: &AppHandle, mate: mm_win32::HWND) {
    let Some(flight) = app.try_state::<crate::flight::Flight>() else {
        return;
    };
    let monitors = mm_win32::monitors::monitors();
    let areas = monitors
        .iter()
        .map(|monitor| monitor.area)
        .collect::<Vec<_>>();
    if !mm_core::presence::lost(flight.position(), &areas) {
        return;
    }
    let Some(home) = crate::overlay::starting_home(app, &monitors) else {
        return;
    };
    // Twice, like overlay.rs: a move onto a monitor of another scale resizes the window.
    if mm_win32::overlay::place(mate, home).is_ok() && mm_win32::overlay::place(mate, home).is_ok()
    {
        flight.dragged(home);
        flight.set_home(home);
    }
}

/// The page menu needs room, but must never activate Mate. Restore its original rectangle.
#[tauri::command]
pub fn mate_menu_size(window: tauri::WebviewWindow, open: bool) -> Result<(), String> {
    if window.label() != "mate" {
        return Err("not the Mate window".into());
    }
    #[cfg(windows)]
    {
        use mm_core::geom::PhysRect;
        let app = window.app_handle();
        let state = app.state::<Presence>();
        let mut saved = state.1.lock().unwrap();
        let hwnd = window.hwnd().map_err(|error| error.to_string())?;
        if open && saved.is_none() {
            let position = window.outer_position().map_err(|error| error.to_string())?;
            let size = window.outer_size().map_err(|error| error.to_string())?;
            let scale = window.scale_factor().map_err(|error| error.to_string())?;
            let original = PhysRect {
                left: position.x,
                top: position.y,
                right: position.x + size.width as i32,
                bottom: position.y + size.height as i32,
            };
            let width = (size.width as i32).max((320.0 * scale).ceil() as i32);
            let height = (size.height as i32).max((280.0 * scale).ceil() as i32);
            let mut rect = PhysRect {
                right: original.left + width,
                bottom: original.top + height,
                ..original
            };
            if let Some(monitor) = mm_win32::monitors::monitors()
                .iter()
                .min_by_key(|m| m.work_area.distance_to(original.center()) as i64)
            {
                let work = monitor.work_area;
                rect.left = rect.left.min(work.right - width).max(work.left);
                rect.top = rect.top.min(work.bottom - height).max(work.top);
                rect.right = rect.left + width;
                rect.bottom = rect.top + height;
            }
            mm_win32::overlay::place(hwnd, rect).map_err(|error| error.to_string())?;
            *saved = Some(original);
            // Clicks in another app do not reach the WebView. Observe the existing native
            // mouse poll without intercepting input or activating either window.
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if handle.state::<Presence>().1.lock().unwrap().is_none() {
                        break;
                    }
                    if let Some((cursor, true)) = mm_win32::mouse::drag_pointer()
                        && !rect.contains(cursor)
                    {
                        let _ = handle.emit_to("mate", "mate-menu-outside", ());
                        break;
                    }
                }
            });
        } else if !open && let Some(original) = *saved {
            mm_win32::overlay::place(hwnd, original).map_err(|error| error.to_string())?;
            *saved = None;
        }
    }
    Ok(())
}
