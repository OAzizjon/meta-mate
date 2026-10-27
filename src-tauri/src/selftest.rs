//! Spike S1 without a person at the desk: run with `--s1-selftest`. The user's
//! foreground window (whatever it was when the app started) must stay the foreground
//! window while Mate is shown, moved across every monitor, and the highlight is shown
//! and hidden for a capture frame. Prints one JSON line and exits 0 (pass) or 1.

use std::thread::sleep;
use std::time::{Duration, Instant};

use mm_win32::HWND;
use mm_win32::overlay::{self, OverlayKind};
use mm_win32::{dpi, focus, monitors};
use serde::Serialize;
use tauri::AppHandle;

use crate::overlay::{Overlays, corner};

#[derive(Serialize)]
struct MonitorReport {
    area: [i32; 4],
    dpi: u32,
    primary: bool,
    mate_dpi: u32,
    foreground_kept: bool,
}

#[derive(Serialize)]
struct Report {
    passed: bool,
    per_monitor_v2: bool,
    had_foreground: bool,
    foreground_kept: bool,
    mate_no_activate: bool,
    highlight_click_through: bool,
    monitors: Vec<MonitorReport>,
    elapsed_ms: u128,
}

pub fn spawn(app: AppHandle, overlays: Overlays, before: HWND, per_monitor_v2: bool) {
    let (Ok(mate), Ok(highlight)) = (overlays.mate.hwnd(), overlays.highlight.hwnd()) else {
        println!("{{\"passed\":false,\"error\":\"no window handles\"}}");
        app.exit(1);
        return;
    };
    // Raw handles cross the thread as numbers; Win32 calls on them are thread-safe.
    let (mate, highlight, before) = (mate.0 as isize, highlight.0 as isize, before.0 as isize);
    std::thread::spawn(move || {
        let handle = |raw: isize| HWND(raw as *mut core::ffi::c_void);
        let report = run(
            handle(mate),
            handle(highlight),
            handle(before),
            per_monitor_v2,
        );
        println!("{}", serde_json::to_string(&report).unwrap_or_default());
        app.exit(if report.passed { 0 } else { 1 });
    });
}

fn run(mate: HWND, highlight: HWND, before: HWND, per_monitor_v2: bool) -> Report {
    let started = Instant::now();
    let kept = || focus::foreground() == before;
    // Let both webviews finish loading: a late WebView2 init is when focus is stolen.
    sleep(Duration::from_millis(1500));
    let mut foreground_kept = kept();

    let mate_no_activate = overlay::ex_style(mate).contains(OverlayKind::Robot.ex_style());
    let highlight_click_through =
        overlay::ex_style(highlight).contains(OverlayKind::Highlight.ex_style());

    let mut reports = Vec::new();
    for monitor in monitors::monitors() {
        let mut ok = false;
        if let Some(rect) = corner(&monitor) {
            ok = overlay::place(mate, rect).is_ok();
            sleep(Duration::from_millis(400));
        }
        let here = ok && kept();
        foreground_kept &= here;
        reports.push(MonitorReport {
            area: [
                monitor.area.left,
                monitor.area.top,
                monitor.area.right,
                monitor.area.bottom,
            ],
            dpi: monitor.dpi,
            primary: monitor.primary,
            mate_dpi: dpi::dpi_for_window(mate),
            foreground_kept: here,
        });
    }

    // The highlight shows over the primary monitor, then hides for one capture frame.
    overlay::show(highlight);
    sleep(Duration::from_millis(300));
    foreground_kept &= kept();
    overlay::hide(highlight);
    sleep(Duration::from_millis(50));
    overlay::show(highlight);
    sleep(Duration::from_millis(300));
    foreground_kept &= kept();

    let dpi_follows = reports.iter().all(|m| m.mate_dpi == m.dpi);
    Report {
        passed: per_monitor_v2
            && foreground_kept
            && mate_no_activate
            && highlight_click_through
            && dpi_follows
            && !reports.is_empty(),
        per_monitor_v2,
        had_foreground: !before.is_invalid(),
        foreground_kept,
        mate_no_activate,
        highlight_click_through,
        monitors: reports,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

/// The control for the self-test: an ordinary Tauri window (focused, shown the usual
/// way) started the same way. If it takes the foreground and ours do not, the
/// self-test's "foreground kept" means something.
pub fn spawn_control(app: AppHandle, before: HWND) {
    let built = tauri::WebviewWindowBuilder::new(
        &app,
        "control",
        tauri::WebviewUrl::App("mate.html".into()),
    )
    .inner_size(200.0, 120.0)
    .build();
    let before = before.0 as isize;
    std::thread::spawn(move || {
        sleep(Duration::from_millis(1500));
        let stolen = focus::foreground().0 as isize != before;
        println!(
            "{{\"control\":true,\"window_built\":{},\"foreground_stolen\":{stolen}}}",
            built.is_ok()
        );
        app.exit(0);
    });
}
