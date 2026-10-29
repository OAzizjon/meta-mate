//! Signing in is required (owner, 2026-10-29): until the person signs in with Google or
//! GitHub, Mate answers nothing and the sign-in window (signin.html, src/signin.ts) is open.
//! The installed app never falls back to the built-in invite; development builds keep the
//! dev invite for the self-tests unless `MM_REQUIRE_SIGN_IN=1`.

use std::sync::atomic::{AtomicBool, Ordering};

use mm_core::constants::APP_NAME;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const WINDOW: &str = "signin";

/// One open at a time: a burst of key presses queues one window, not one each.
static OPENING: AtomicBool = AtomicBool::new(false);

/// Release builds always; development builds only with `MM_REQUIRE_SIGN_IN=1`.
pub(crate) fn required() -> bool {
    cfg!(not(debug_assertions))
        || std::env::var("MM_REQUIRE_SIGN_IN").is_ok_and(|value| value.trim() == "1")
}

/// The invite a proxy client may carry: none while signing in is required, so a call
/// without a session is refused by the proxy instead of riding on the shared invite.
pub(crate) fn invite_for_proxy(invite: String) -> String {
    if required() { String::new() } else { invite }
}

/// Whether a new question must wait for a sign-in.
pub(crate) fn missing(app: &AppHandle) -> bool {
    required()
        && !app
            .try_state::<crate::auth::Auth>()
            .is_some_and(|auth| auth.signed_in())
}

/// A normal window the person clicks into; shown without taking the keyboard from the
/// person's app (invariant 1), like Home.
pub(crate) fn open(app: &AppHandle) -> tauri::Result<()> {
    // A queued open can run after the sign-in it asked for has already landed.
    if !missing(app) {
        close(app);
        return Ok(());
    }
    let window = match app.get_webview_window(WINDOW) {
        Some(window) => window,
        None => WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("signin.html".into()))
            .title(APP_NAME)
            .inner_size(560.0, 600.0)
            .center()
            .resizable(true)
            .decorations(true)
            .focused(false)
            .visible(false)
            .additional_browser_args(crate::overlay::BROWSER_ARGS)
            .build()?,
    };
    #[cfg(windows)]
    mm_win32::overlay::show(window.hwnd()?);
    #[cfg(not(windows))]
    window.show()?;
    // Signed in while the window was being built: it closes again.
    if !missing(app) {
        close(app);
    }
    Ok(())
}

/// Opens the window off the event thread (window construction there can deadlock WebView2).
pub(crate) fn open_later(app: &AppHandle) {
    if OPENING.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = open(&app) {
            eprintln!("signin: window: {error}");
        }
        OPENING.store(false, Ordering::Release);
    });
}

/// After a successful sign-in the window has done its job.
pub(crate) fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(debug_assertions)]
    #[test]
    fn development_builds_keep_the_dev_invite_unless_asked() {
        // SAFETY: tests in this module are the only readers of this variable.
        unsafe { std::env::remove_var("MM_REQUIRE_SIGN_IN") };
        assert!(!required());
        assert_eq!(invite_for_proxy("dev".into()), "dev");
        unsafe { std::env::set_var("MM_REQUIRE_SIGN_IN", "1") };
        assert!(required());
        assert_eq!(invite_for_proxy("dev".into()), "");
        unsafe { std::env::remove_var("MM_REQUIRE_SIGN_IN") };
    }
}
