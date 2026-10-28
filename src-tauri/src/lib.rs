//! MetaMate's Tauri shell: wiring only, no business logic (docs/project-structure.md).
//!
//! `--s1-selftest` runs spike S1 unattended (docs/architecture.md section 12): it shows
//! Mate and the highlight, moves Mate across every monitor, checks that the user's
//! foreground window never changed and that DPI follows each monitor, prints a JSON
//! report and exits with 0 (pass) or 1. `--voice-selftest FILE.wav` runs the voice path
//! against the dev proxy without any window (voice_selftest.rs). `--ptt-probe` prints
//! push-to-talk events for 60 s without any window (spike S2, ptt.rs). `--ask "question"`
//! plays one answer in Mate's window and exits (spike S7, voice.rs). `--card-selftest [ru]`
//! makes one lesson card through the dev proxy and exits (card.rs). A normal start
//! in a development build also runs the live voice loop (voice.rs) when
//! `server/run_dev.py --tts` is up.

mod answer_stream;
mod auth;
mod card;
#[cfg(debug_assertions)]
mod debug_view;
#[cfg(windows)]
mod flight;
#[cfg(windows)]
mod highlight;
mod mic;
mod overlay;
#[cfg(windows)]
mod pointing;
mod proxy;
#[cfg(windows)]
mod ptt;
#[cfg(windows)]
mod s3;
#[cfg(windows)]
mod sam;
#[cfg(windows)]
mod screen;
#[cfg(windows)]
mod selftest;
mod settings;
#[cfg(windows)]
mod teacher;
#[cfg(windows)]
mod voice;
mod voice_selftest;

/// Starts the app. Errors at startup are printed and end the process with code 1.
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = voice_selftest::run_if_requested(&args) {
        std::process::exit(code);
    }
    #[cfg(windows)]
    if args.iter().any(|arg| arg == "--look-probe") {
        println!("{}", screen::probe());
        std::process::exit(0);
    }
    #[cfg(windows)]
    if let Some(code) = ptt::probe_if_requested(&args) {
        std::process::exit(code);
    }
    #[cfg(windows)]
    let per_monitor_v2 = mm_win32::dpi::ensure_per_monitor_v2();
    let selftest = std::env::args().any(|arg| arg == "--s1-selftest");
    let control = std::env::args().any(|arg| arg == "--s1-control");
    let s3 = std::env::args().any(|arg| arg == "--s3-selftest");
    let ask = args
        .iter()
        .position(|arg| arg == "--ask")
        .and_then(|at| args.get(at + 1))
        .cloned();
    let card_selftest = args
        .iter()
        .position(|arg| arg == "--card-selftest")
        .map(|at| args.get(at + 1).is_some_and(|language| language == "ru"));

    let result = tauri::Builder::default()
        .manage(answer_stream::AnswerStream::default())
        .manage(pointing::Pointing::default())
        .manage(auth::Auth::default())
        .invoke_handler(tauri::generate_handler![
            answer_stream::subscribe_answer_stream,
            answer_stream::report_playback,
            pointing::point_at,
            settings::load_settings,
            settings::save_settings,
            auth::auth_status,
            auth::sign_in,
            auth::sign_out
        ])
        .setup(move |app| {
            {
                use tauri::Manager;
                app.manage(settings::SettingsStore::new(settings::load(app.handle())));
            }
            #[cfg(windows)]
            let before = mm_win32::focus::foreground();
            #[cfg(windows)]
            if s3 {
                s3::spawn(app.handle().clone(), before);
                return Ok(());
            }
            #[cfg(windows)]
            if control {
                selftest::spawn_control(app.handle().clone(), before);
                return Ok(());
            }
            #[cfg(all(windows, debug_assertions))]
            if let Some(russian) = card_selftest {
                card::selftest(app.handle(), russian);
                return Ok(());
            }
            #[cfg(debug_assertions)]
            {
                use tauri::Manager;
                // `--debug-window`: the live timeline, timings and looks (debug.html).
                app.state::<answer_stream::AnswerStream>()
                    .attach_debug(app.handle().clone());
                debug_view::open(app.handle());
            }
            let overlays = overlay::create(app.handle())?;
            #[cfg(windows)]
            if selftest {
                selftest::spawn(app.handle().clone(), overlays, before, per_monitor_v2);
                return Ok(());
            }
            #[cfg(windows)]
            {
                use tauri::Manager;
                // Mate flies beside a target (pointing.rs); each monitor's highlight window
                // is shown once, empty: transparent, click-through, never activated
                // (invariant 1).
                let home = overlay::home(&mm_win32::monitors::monitors());
                if let (Ok(mate), Some(home)) = (overlays.mate.hwnd(), home) {
                    app.manage(flight::Flight::new(mate, home));
                }
                for (highlight, monitor) in &overlays.highlights {
                    if let Ok(highlight) = highlight.hwnd() {
                        mm_win32::overlay::show(highlight);
                        // Placed again once shown, in case the scale change of its monitor
                        // only arrived now (overlay.rs).
                        let _ = mm_win32::overlay::place(highlight, monitor.area);
                    }
                }
            }
            #[cfg(windows)]
            if let Some(question) = ask {
                voice::ask(app.handle(), question);
                return Ok(());
            }
            // Without the tray Mate still works: no reason to stop.
            if let Err(error) = tray(app.handle()) {
                eprintln!("tray: {error}");
            }
            #[cfg(windows)]
            if let Some(voice) = voice::start(app.handle()) {
                use tauri::Manager;
                app.manage(voice);
            }
            #[cfg(not(windows))]
            let _ = (overlays, selftest, control, s3);
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("{}: could not start: {error}", mm_core::constants::APP_NAME);
        std::process::exit(1);
    }
}

/// Tray menu ids.
const MENU_SETTINGS: &str = "settings";
const MENU_QUIT: &str = "quit";
/// The settings window's label (capabilities/default.json) and page (src/settings.ts).
const SETTINGS_WINDOW: &str = "settings";
const SETTINGS_PAGE: &str = "settings.html";

/// The tray icon (the app icon) with "Settings" and "Quit", in the UI locale Mate's window
/// reports (its `navigator.language` follows the Windows display language): English until
/// that window subscribes, then relabelled once.
fn tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use std::time::{Duration, Instant};
    use tauri::Manager;
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let locale = app.state::<answer_stream::AnswerStream>().locale();
    let settings_item = MenuItem::with_id(
        app,
        MENU_SETTINGS,
        locale_text(&locale, "tray.settings"),
        true,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(
        app,
        MENU_QUIT,
        locale_text(&locale, "tray.quit"),
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&settings_item, &quit_item])?;
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip(format!(
            "{}: {}",
            mm_core::constants::APP_NAME,
            locale_text(&locale, "tray.tooltip")
        ))
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_SETTINGS => {
                // Building a window inside an event handler deadlocks on Windows (WebView2):
                // the async runtime builds it.
                let app = app.clone();
                tauri::async_runtime::spawn(async move { open_settings(&app) });
            }
            MENU_QUIT => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    // The app keeps the icon; this handle relabels its tooltip below.
    let tray = builder.build(app)?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let stream = app.state::<answer_stream::AnswerStream>();
        let started = Instant::now();
        while !stream.subscribed() && started.elapsed() < Duration::from_secs(15) {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let locale = stream.locale();
        let _ = settings_item.set_text(locale_text(&locale, "tray.settings"));
        let _ = quit_item.set_text(locale_text(&locale, "tray.quit"));
        let _ = tray.set_tooltip(Some(format!(
            "{}: {}",
            mm_core::constants::APP_NAME,
            locale_text(&locale, "tray.tooltip")
        )));
    });
    Ok(())
}

/// Opens the settings window (560x720, centred), or brings it forward when it is open:
/// one instance. A normal window the person asked for from the tray, so it takes focus;
/// invariant 1 is about Mate's windows, which never do.
fn open_settings(app: &tauri::AppHandle) {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let locale = app.state::<answer_stream::AnswerStream>().locale();
    let title = format!(
        "{} {}",
        mm_core::constants::APP_NAME,
        locale_text(&locale, "tray.settings")
    );
    // Two quick clicks may both get here: the second build fails on the taken label.
    let result =
        WebviewWindowBuilder::new(app, SETTINGS_WINDOW, WebviewUrl::App(SETTINGS_PAGE.into()))
            .title(title)
            .inner_size(560.0, 720.0)
            .center()
            .additional_browser_args(overlay::BROWSER_ARGS)
            .build();
    if let Err(error) = result {
        eprintln!("settings: window: {error}");
    }
}

/// `key` from the locale files (invariant 7): Russian for the "ru" locale, else English;
/// the key itself when it is missing.
pub(crate) fn locale_text(locale: &str, key: &str) -> String {
    let strings = if locale == "ru" {
        include_str!("../../locales/ru.json")
    } else {
        include_str!("../../locales/en.json")
    };
    serde_json::from_str::<serde_json::Value>(strings)
        .ok()
        .and_then(|strings| strings.get(key)?.as_str().map(str::to_owned))
        .unwrap_or_else(|| key.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tray_menu_has_both_languages() {
        for locale in ["en", "ru"] {
            for key in ["tray.settings", "tray.quit"] {
                let text = locale_text(locale, key);
                assert!(!text.is_empty() && text != key, "{locale} {key}");
            }
        }
        assert_eq!(locale_text("en", "tray.quit"), "Quit");
        assert_ne!(locale_text("ru", "tray.quit"), "Quit");
        // Any other locale is English.
        assert_eq!(locale_text("de", "tray.settings"), "Settings");
    }
}
