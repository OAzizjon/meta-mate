//! Signing in is required (owner, 2026-10-29): until the person signs in with Google or
//! GitHub, Mate answers nothing and the sign-in window (signin.html, src/signin.ts) is open.
//! The installed app never falls back to the built-in invite; development builds keep the
//! dev invite for the self-tests unless `MM_REQUIRE_SIGN_IN=1`.
//!
//! The guest trial (owner, 2026-10-29): without signing in, a device gets a guest token and
//! 12 answers an hour (counted by the proxy); the count shows in Mate's window and here.

use std::sync::atomic::{AtomicBool, Ordering};

use mm_core::constants::APP_NAME;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::proxy::{GuestUsage, Proxy};

const DEVICE_FILE: &str = "device.id";
const USAGE_EVENT: &str = "guest-usage";

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

fn signed_in(app: &AppHandle) -> bool {
    app.try_state::<crate::auth::Auth>()
        .is_some_and(|auth| auth.signed_in())
}

/// Whether the guest trial is used up for this hour (signed out, guest token, 0 left).
pub(crate) fn guest_used_up(app: &AppHandle) -> bool {
    !signed_in(app)
        && app.try_state::<Proxy>().is_some_and(|proxy| {
            proxy.has_guest() && proxy.guest_usage().is_some_and(|u| u.used_up())
        })
}

/// Whether a new question must wait for a sign-in: signed out and no guest answer left.
pub(crate) fn missing(app: &AppHandle) -> bool {
    if !required() || signed_in(app) {
        return false;
    }
    let guest = app
        .try_state::<Proxy>()
        .is_some_and(|proxy| proxy.has_guest());
    !guest || guest_used_up(app)
}

/// The line Mate says when a question waits for a sign-in.
pub(crate) fn refusal_line(app: &AppHandle) -> &'static str {
    if guest_used_up(app) {
        "robot.guest_limit"
    } else {
        "robot.sign_in_first"
    }
}

/// Signed out: a guest token for this device, then every count goes to the windows (and an
/// empty hour opens this window). Failing quietly keeps the plain sign-in.
pub(crate) fn start_guest(app: &AppHandle, proxy: &Proxy) {
    if !required() || signed_in(app) {
        return;
    }
    let app = app.clone();
    let proxy = proxy.clone();
    tauri::async_runtime::spawn(async move {
        // Without a trial (an older proxy, no data folder) the plain sign-in remains.
        let token = match device_id(&app) {
            Some(device) => proxy.guest_token(&device).await.map_err(|e| e.to_string()),
            None => Err("no device id".to_owned()),
        };
        match token {
            Ok(token) => proxy.set_guest(Some(token)),
            Err(error) => {
                eprintln!("signin: guest trial: {error}");
                open_later(&app);
                return;
            }
        }
        // Guest tokens live a day: a fresh one twice a day while still signed out.
        let renew = (app.clone(), proxy.clone());
        tauri::async_runtime::spawn(async move {
            let (app, proxy) = renew;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(12 * 3600)).await;
                if signed_in(&app) || !proxy.has_guest() {
                    break;
                }
                if let Some(device) = device_id(&app)
                    && let Ok(token) = proxy.guest_token(&device).await
                {
                    proxy.set_guest(Some(token));
                }
            }
        });
        let mut usage = proxy.watch_guest_usage();
        let mut was_left = None;
        loop {
            let current = *usage.borrow_and_update();
            if let Some(current) = current {
                let _ = app.emit(USAGE_EVENT, current);
                // Once, when the hour runs out: closing the window is not undone by the next count.
                if current.left == 0 && was_left != Some(0) && !signed_in(&app) {
                    open_later(&app);
                }
                was_left = Some(current.left);
            }
            if usage.changed().await.is_err() {
                break;
            }
        }
    });
}

/// A random id kept in the app's data folder: the proxy counts the trial per device.
fn device_id(app: &AppHandle) -> Option<String> {
    let dir = app.path().app_local_data_dir().ok()?;
    let path = dir.join(DEVICE_FILE);
    if let Ok(kept) = std::fs::read_to_string(&path) {
        let kept = kept.trim();
        if (22..=64).contains(&kept.len())
            && kept
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Some(kept.to_owned());
        }
    }
    let id = crate::auth::random_url_safe::<24>()?;
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::write(&path, &id).ok()?;
    Some(id)
}

/// The guest count for the sign-in window and Mate's window (`None`: no trial).
#[tauri::command]
pub fn guest_usage(app: AppHandle) -> Option<GuestUsage> {
    if !required() || signed_in(&app) {
        return None;
    }
    app.try_state::<Proxy>()
        .filter(|proxy| proxy.has_guest())
        .and_then(|proxy| proxy.guest_usage())
}

/// "Try without signing in": the window closes while trial answers are left.
#[tauri::command]
pub fn try_as_guest(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != WINDOW {
        return Err("not the sign-in window".into());
    }
    if missing(&app) {
        return Err("signin.guest_none".into());
    }
    close(&app);
    Ok(())
}

/// A normal window the person clicks into; shown without taking the keyboard from the
/// person's app (invariant 1), like Home.
pub(crate) fn open(app: &AppHandle) -> tauri::Result<()> {
    // A queued open can run after the sign-in it asked for has already landed.
    if signed_in(app) {
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
    if signed_in(app) {
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
