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
                .unwrap_or_else(std::sync::PoisonError::into_inner)
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

/// The scam fire drill (practice.html): fictional scam pages where the person spots the red
/// flags. Opened from Home like the chat: an explicit open, so it comes to the front.
#[tauri::command]
pub async fn open_practice(app: AppHandle) -> Result<(), String> {
    let window = match app.get_webview_window("practice") {
        Some(window) => window,
        None => {
            WebviewWindowBuilder::new(&app, "practice", WebviewUrl::App("practice.html".into()))
                .title(crate::locale_text(
                    &app.state::<crate::answer_stream::AnswerStream>().locale(),
                    "practice.title",
                ))
                .inner_size(980.0, 720.0)
                .min_inner_size(560.0, 480.0)
                .center()
                .resizable(true)
                .decorations(true)
                .focused(false)
                .visible(false)
                .additional_browser_args(crate::overlay::BROWSER_ARGS)
                .build()
                .map_err(|error| {
                    eprintln!("practice: window: {error}");
                    "could not open the practice window".to_owned()
                })?
        }
    };
    let _ = window.unminimize();
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|error| error.to_string())?;
        mm_win32::overlay::show(hwnd);
        mm_win32::focus::activate_on_request(hwnd);
    }
    #[cfg(not(windows))]
    window.show().map_err(|error| error.to_string())?;
    Ok(())
}

fn validate_try_key(key: &str) -> Result<(), String> {
    if TRY_QUESTIONS.contains(&key) {
        Ok(())
    } else {
        Err("unknown sample question".to_owned())
    }
}

#[cfg(any(windows, test))]
#[derive(Debug, PartialEq, Eq)]
enum TryAction {
    Display,
    Ask,
    OpenBrowser,
}

#[cfg(any(windows, test))]
fn try_action(key: &str, has_target: bool) -> TryAction {
    if key == "tray.try_bigger" {
        TryAction::Display
    } else if has_target {
        TryAction::Ask
    } else {
        TryAction::OpenBrowser
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
        match try_action(&key, crate::voice::remembered_target(&app).is_some()) {
            TryAction::Display => {
                if !open_display(&app).await {
                    crate::voice::show_line(&app, "robot.cant_find");
                    return Ok(());
                }
            }
            TryAction::OpenBrowser => {
                crate::voice::show_line(&app, "robot.open_browser_first");
                return Ok(());
            }
            TryAction::Ask => {}
        }
        crate::voice::ask_text(&app, crate::locale_text(&locale, &key));
    }
    #[cfg(not(windows))]
    let _ = app;
    Ok(())
}

/// Wait for the voice loop's remembered target to catch up with Settings activation.
#[cfg(windows)]
async fn open_display(app: &AppHandle) -> bool {
    use std::time::{Duration, Instant};
    let started = Instant::now();
    if !mm_win32::shell::open_url("ms-settings:display") {
        return false;
    }
    loop {
        if crate::voice::remembered_target(app).is_some_and(mm_win32::window::settings_foreground) {
            return true;
        }
        if started.elapsed() >= Duration::from_secs(3) {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
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
    fn display_opens_settings_and_other_samples_need_a_target() {
        for has_target in [false, true] {
            assert_eq!(
                try_action("tray.try_bigger", has_target),
                TryAction::Display
            );
        }
        for key in ["tray.try_search", "tray.try_safe"] {
            assert_eq!(try_action(key, false), TryAction::OpenBrowser);
            assert_eq!(try_action(key, true), TryAction::Ask);
        }
    }

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
