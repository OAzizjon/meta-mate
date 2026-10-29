//! The Home window: instructions and sample questions, shown without taking focus.

use mm_core::constants::APP_NAME;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub(crate) const TRY_QUESTIONS: [&str; 3] = ["tray.try_search", "tray.try_safe", "tray.try_bigger"];

/// A normal window the person may click into; both first show and reopening keep the
/// keyboard in the person's app, like the lesson card (invariant 1).
pub(crate) fn open(app: &AppHandle) -> tauri::Result<()> {
    crate::presence::wake(app, false);
    let window = match app.get_webview_window("home") {
        Some(window) => window,
        None => WebviewWindowBuilder::new(app, "home", WebviewUrl::App("home.html".into()))
            .title(APP_NAME)
            .inner_size(720.0, 640.0)
            .center()
            .resizable(true)
            .decorations(true)
            .focused(false)
            .visible(false)
            .additional_browser_args(crate::overlay::BROWSER_ARGS)
            .build()?,
    };
    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
            let first = handle
                .state::<crate::presence::Presence>()
                .0
                .lock()
                .unwrap()
                .close_home();
            if first {
                use tauri::Emitter;
                let _ = handle.emit_to("mate", "home-closed-hint", ());
            }
        }
    });
    #[cfg(windows)]
    mm_win32::overlay::show(window.hwnd()?);
    #[cfg(not(windows))]
    window.show()?;
    Ok(())
}

/// Window construction runs off the event thread to avoid a WebView2 deadlock.
#[tauri::command]
pub async fn open_home(app: AppHandle) -> Result<(), String> {
    open(&app).map_err(|error| {
        eprintln!("home: window: {error}");
        "could not open home window".to_owned()
    })
}

fn validate_try_key(key: &str) -> Result<(), String> {
    if TRY_QUESTIONS.contains(&key) {
        Ok(())
    } else {
        Err("unknown sample question".to_owned())
    }
}

/// Uses the tray's voice path, including its remembered app target. That path rejects
/// every window belonging to our process, including Home. Home lies over that app and the
/// copy reads the screen, so Home is minimized before the question is asked.
#[tauri::command]
pub async fn home_try(app: AppHandle, key: String) -> Result<(), String> {
    validate_try_key(&key)?;
    #[cfg(windows)]
    {
        if let Some(home) = app.get_webview_window("home") {
            minimize(&home).await;
        }
        let locale = app.state::<crate::answer_stream::AnswerStream>().locale();
        crate::voice::ask_text(&app, crate::locale_text(&locale, &key));
    }
    #[cfg(not(windows))]
    let _ = app;
    Ok(())
}

/// About as long as Windows' minimize animation, which draws Home until it ends.
#[cfg(windows)]
const MINIMIZE_ANIMATION: std::time::Duration = std::time::Duration::from_millis(250);

/// Minimizes Home and returns once it is iconic (`IsIconic`) and its animation is over,
/// after 1 s at most.
#[cfg(windows)]
async fn minimize(home: &tauri::WebviewWindow) {
    use std::time::{Duration, Instant};
    if home.minimize().is_err() {
        return;
    }
    let started = Instant::now();
    tokio::time::sleep(MINIMIZE_ANIMATION).await;
    while !home.is_minimized().unwrap_or(true) && started.elapsed() < Duration::from_secs(1) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_three_tray_questions_are_allowed() {
        for key in ["tray.try_search", "tray.try_safe", "tray.try_bigger"] {
            assert_eq!(validate_try_key(key), Ok(()), "{key}");
        }
        for key in [
            "",
            "tray.settings",
            "tray.quit",
            "tray.try_title",
            "Where is the search box?",
            "tray.try_search ",
            " tray.try_safe",
            "TRAY.TRY_BIGGER",
            "tray.try_search\0",
        ] {
            assert!(validate_try_key(key).is_err(), "{key:?}");
        }
    }
}
