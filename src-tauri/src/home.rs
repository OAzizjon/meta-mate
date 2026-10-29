//! The Home window: instructions and sample questions, shown without taking focus.

use mm_core::constants::APP_NAME;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub(crate) const TRY_QUESTIONS: [&str; 3] = ["tray.try_search", "tray.try_safe", "tray.try_bigger"];

/// A normal window the person may click into; both first show and reopening keep the
/// keyboard in the person's app, like the lesson card (invariant 1).
pub(crate) fn open(app: &AppHandle) -> tauri::Result<()> {
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
/// every window belonging to our process, including Home.
#[tauri::command]
pub fn home_try(app: AppHandle, key: String) -> Result<(), String> {
    validate_try_key(&key)?;
    #[cfg(windows)]
    {
        let locale = app.state::<crate::answer_stream::AnswerStream>().locale();
        crate::voice::ask_text(&app, crate::locale_text(&locale, &key));
    }
    #[cfg(not(windows))]
    let _ = app;
    Ok(())
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
