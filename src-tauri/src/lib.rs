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
mod chat;
#[cfg(debug_assertions)]
mod debug_view;
#[cfg(windows)]
mod drag;
#[cfg(windows)]
mod flight;
#[cfg(windows)]
mod highlight;
mod home;
mod mic;
mod overlay;
#[cfg(debug_assertions)]
mod perf;
#[cfg(windows)]
mod pointing;
mod presence;
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
mod signin;
#[cfg(windows)]
mod teacher;
#[cfg(windows)]
mod voice;
mod voice_selftest;

use home::TRY_QUESTIONS;

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
        .manage(chat::Chat::default())
        .manage(pointing::Pointing::default())
        .manage(auth::Auth::default())
        .manage(presence::Presence::default())
        .on_menu_event(handle_menu)
        .invoke_handler(tauri::generate_handler![
            answer_stream::subscribe_answer_stream,
            answer_stream::report_playback,
            pointing::point_at,
            drag::mate_drag_start,
            settings::load_settings,
            settings::save_settings,
            home::open_home,
            home::home_try,
            chat::open_chat,
            chat::chat_send,
            chat::chat_subscribe,
            chat::chat_clear,
            presence::quit_app,
            presence::hide_mate,
            presence::mate_menu_size,
            open_settings_window,
            auth::auth_status,
            auth::sign_in,
            auth::sign_out,
            signin::guest_usage,
            signin::try_as_guest
        ])
        .setup(move |app| {
            {
                use tauri::Manager;
                app.manage(settings::SettingsStore::new(settings::load(app.handle())));
                chat::init(app.handle());
                app.state::<answer_stream::AnswerStream>()
                    .attach_chat(app.handle().clone());
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
            let overlays = overlay::create_mate(app.handle())?;
            #[cfg(windows)]
            {
                use tauri::Manager;
                let home = overlay::starting_home(app.handle(), &mm_win32::monitors::monitors());
                if let (Ok(mate), Some(home)) = (overlays.mate.hwnd(), home) {
                    app.manage(flight::Flight::new(mate, home));
                }
                let handle = app.handle().clone();
                let before = before.0 as isize;
                tauri::async_runtime::spawn(async move {
                    let mut overlays = overlays;
                    match overlay::create_highlights(&handle) {
                        Ok(highlights) => overlays.highlights = highlights,
                        Err(error) => {
                            eprintln!("overlay: highlights: {error}");
                            handle.exit(1);
                            return;
                        }
                    }
                    if selftest {
                        selftest::spawn(
                            handle,
                            overlays,
                            mm_win32::HWND(before as *mut std::ffi::c_void),
                            per_monitor_v2,
                        );
                        return;
                    }
                    for (highlight, monitor) in &overlays.highlights {
                        if let Ok(hwnd) = highlight.hwnd() {
                            mm_win32::overlay::show(hwnd);
                            // A delayed DPI notification may arrive only after showing.
                            let _ = mm_win32::overlay::place(hwnd, monitor.area);
                        }
                    }
                    #[cfg(debug_assertions)]
                    {
                        handle
                            .state::<answer_stream::AnswerStream>()
                            .attach_debug(handle.clone());
                        debug_view::open(&handle);
                    }
                    if let Some(question) = ask {
                        voice::ask(&handle, question);
                        return;
                    }
                    finish_startup(&handle);
                });
            }
            #[cfg(windows)]
            if selftest {
                return Ok(());
            }
            #[cfg(not(windows))]
            finish_startup(app.handle());
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

/// Normal startup follows prepared highlights, so early Home/tray requests use
/// the same fully initialized voice and capture paths as later requests.
fn finish_startup(app: &tauri::AppHandle) {
    // Without the tray Mate still works: no reason to stop.
    if let Err(error) = tray(app) {
        eprintln!("tray: {error}");
    }
    #[cfg(windows)]
    if let Some(voice) = voice::start(app) {
        use tauri::Manager;
        app.manage(voice);
    }
    {
        use tauri::Manager;
        // Signing in is required: before Mate answers, the sign-in window opens
        // (after Home, so it is the one on top).
        let sign_in = signin::required() && !app.state::<auth::Auth>().signed_in();
        if app
            .state::<settings::SettingsStore>()
            .get()
            .show_home_at_start
        {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = home::open(&app) {
                    eprintln!("home: window: {error}");
                }
                if sign_in && let Err(error) = signin::open(&app) {
                    eprintln!("signin: window: {error}");
                }
            });
        } else if sign_in {
            signin::open_later(app);
        }
    }
}

/// Tray menu ids.
const MENU_HOME: &str = "tray.open_home";
const MENU_CHAT: &str = "tray.chat";
const MENU_SETTINGS: &str = "settings";
const MENU_QUIT: &str = "quit";
const MENU_HIDE: &str = "hide_hour";
const MENU_WAKE: &str = "wake";
/// The settings window's label (capabilities/default.json) and page (src/settings.ts).
const SETTINGS_WINDOW: &str = "settings";
const SETTINGS_PAGE: &str = "settings.html";

/// The tray's "Bring Mate back now": in its menu only while Mate is hidden.
struct TrayWake {
    menu: tauri::menu::Menu<tauri::Wry>,
    item: tauri::menu::MenuItem<tauri::Wry>,
}

/// Puts "Bring Mate back now" at the top of the tray menu while Mate is hidden. Removed
/// first, so hiding twice does not list it twice.
pub(crate) fn show_tray_wake(app: &tauri::AppHandle, hidden: bool) {
    use tauri::Manager;
    if let Some(wake) = app.try_state::<TrayWake>() {
        let _ = wake.menu.remove(&wake.item);
        if hidden {
            let _ = wake.menu.insert(&wake.item, 0);
        }
    }
}

/// The tray menu follows the saved language preference. Automatic uses the Windows
/// display language reported by Mate's WebView.
fn tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::Manager;
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    use tauri::tray::TrayIconBuilder;

    let locale = app.state::<answer_stream::AnswerStream>().locale();
    let chat_item = MenuItem::with_id(
        app,
        MENU_CHAT,
        locale_text(&locale, MENU_CHAT),
        true,
        None::<&str>,
    )?;
    let home_item = MenuItem::with_id(
        app,
        MENU_HOME,
        locale_text(&locale, MENU_HOME),
        true,
        None::<&str>,
    )?;
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
    let hide_item = MenuItem::with_id(
        app,
        MENU_HIDE,
        locale_text(&locale, "tray.hide_hour"),
        true,
        None::<&str>,
    )?;
    let wake_item = MenuItem::with_id(
        app,
        MENU_WAKE,
        locale_text(&locale, "tray.wake"),
        true,
        None::<&str>,
    )?;
    let try_items = TRY_QUESTIONS
        .iter()
        .map(|key| MenuItem::with_id(app, *key, locale_text(&locale, key), true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let try_menu = Submenu::with_items(
        app,
        locale_text(&locale, "tray.try_title"),
        true,
        &try_items
            .iter()
            .map(|item| item as &dyn tauri::menu::IsMenuItem<_>)
            .collect::<Vec<_>>(),
    )?;
    let menu = Menu::with_items(
        app,
        &[
            &chat_item,
            &home_item,
            &try_menu,
            &settings_item,
            &hide_item,
            &PredefinedMenuItem::separator(app)?,
            &quit_item,
        ],
    )?;
    app.manage(TrayWake {
        menu: menu.clone(),
        item: wake_item.clone(),
    });
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip(tray_tooltip(app));
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    // The app keeps the icon; this handle relabels its tooltip below.
    let _tray = builder.build(app)?;
    app.manage(TrayLabels {
        items: vec![
            (chat_item, MENU_CHAT),
            (home_item, MENU_HOME),
            (settings_item, "tray.settings"),
            (hide_item, "tray.hide_hour"),
            (wake_item, "tray.wake"),
            (quit_item, "tray.quit"),
        ]
        .into_iter()
        .chain(try_items.into_iter().zip(TRY_QUESTIONS.iter().copied()))
        .collect(),
        try_menu,
    });
    Ok(())
}

struct TrayLabels {
    items: Vec<(tauri::menu::MenuItem<tauri::Wry>, &'static str)>,
    try_menu: tauri::menu::Submenu<tauri::Wry>,
}

pub(crate) fn refresh_tray_labels(app: &tauri::AppHandle) {
    use tauri::Manager;
    let locale = app.state::<answer_stream::AnswerStream>().locale();
    if let Some(labels) = app.try_state::<TrayLabels>() {
        for (item, key) in &labels.items {
            let _ = item.set_text(locale_text(&locale, key));
        }
        let _ = labels
            .try_menu
            .set_text(locale_text(&locale, "tray.try_title"));
    }
    refresh_tray_tooltip(app);
}

fn tray_tooltip(app: &tauri::AppHandle) -> String {
    use tauri::Manager;
    let locale = app.state::<answer_stream::AnswerStream>().locale();
    let key = app.state::<settings::SettingsStore>().get().ptt_key;
    format!(
        "{}: {}",
        mm_core::constants::APP_NAME,
        locale_text(&locale, "tray.tooltip")
            .replace("{key}", &locale_text(&locale, key.locale_key()))
    )
}

pub(crate) fn refresh_tray_tooltip(app: &tauri::AppHandle) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tray_tooltip(app)));
    }
}

/// Tray events use the tray's own native host, never Mate's HWND as a popup owner.
fn handle_menu(app: &tauri::AppHandle, event: tauri::menu::MenuEvent) {
    use tauri::Manager;
    match event.id.as_ref() {
        MENU_CHAT => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if chat::open_chat(app).await.is_err() {
                    eprintln!("chat: could not open window");
                }
            });
        }
        MENU_HOME => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = home::open(&app) {
                    eprintln!("home: window: {error}");
                }
            });
        }
        MENU_SETTINGS => {
            // Building a window inside an event handler deadlocks on Windows (WebView2):
            // the async runtime builds it.
            let app = app.clone();
            tauri::async_runtime::spawn(async move { open_settings(&app) });
        }
        MENU_QUIT => app.exit(0),
        MENU_HIDE => presence::hide(app),
        MENU_WAKE => presence::wake(app, false),
        #[cfg(windows)]
        key if TRY_QUESTIONS.contains(&key) => {
            let text = locale_text(&app.state::<answer_stream::AnswerStream>().locale(), key);
            voice::ask_text(app, text);
        }
        _ => {}
    }
}

/// Shares the tray's Settings action, building the WebView off the event thread.
#[tauri::command]
async fn open_settings_window(app: tauri::AppHandle) {
    open_settings(&app);
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
            for key in [
                MENU_HOME,
                "tray.settings",
                "tray.quit",
                "tray.try_title",
                "tray.hide_hour",
                "tray.wake",
                "home.quit",
                "home.closed_hint",
                "robot.no_microphone",
                "robot.open_browser_first",
            ]
            .into_iter()
            .chain(TRY_QUESTIONS)
            {
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
