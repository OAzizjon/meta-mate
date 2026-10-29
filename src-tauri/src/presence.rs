//! Mate's native menu and session visibility. Window changes run on the event thread.

use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Manager};

#[derive(Default)]
pub struct Presence(pub Mutex<mm_core::presence::Presence>);

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

pub fn hide(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window("mate") else {
            return;
        };
        if window.hide().is_err() {
            return;
        }
        handle
            .state::<Presence>()
            .0
            .lock()
            .unwrap()
            .hide(Instant::now());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(mm_core::presence::HIDE_DURATION).await;
            wake(&handle, true);
        });
    });
}

pub fn wake(app: &AppHandle, timer: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = handle.state::<Presence>();
        let mut state = state.0.lock().unwrap();
        let show = if timer {
            state.wake_if_due(Instant::now())
        } else {
            state.wake()
        };
        if show && let Some(window) = handle.get_webview_window("mate") {
            #[cfg(windows)]
            if let Ok(hwnd) = window.hwnd() {
                mm_win32::overlay::show(hwnd);
            }
            #[cfg(not(windows))]
            let _ = window.show();
        }
    });
}

/// The overlay stays unfocusable; do not focus it to display the native popup.
#[tauri::command]
pub fn mate_context_menu(window: tauri::WebviewWindow) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem};
    if window.label() != "mate" {
        return Err("not the Mate window".into());
    }
    let app = window.app_handle();
    let locale = app.state::<crate::answer_stream::AnswerStream>().locale();
    let items = [
        (crate::MENU_HOME, crate::MENU_HOME),
        (crate::MENU_SETTINGS, "tray.settings"),
        (crate::MENU_HIDE, "tray.hide_hour"),
        (crate::MENU_QUIT, "home.quit"),
    ]
    .into_iter()
    .map(|(id, key)| {
        MenuItem::with_id(
            app,
            id,
            crate::locale_text(&locale, key),
            true,
            None::<&str>,
        )
    })
    .collect::<tauri::Result<Vec<_>>>()
    .map_err(|error| error.to_string())?;
    let menu = Menu::with_items(
        app,
        &items
            .iter()
            .map(|item| item as &dyn tauri::menu::IsMenuItem<_>)
            .collect::<Vec<_>>(),
    )
    .map_err(|error| error.to_string())?;
    window.popup_menu(&menu).map_err(|error| error.to_string())
}
