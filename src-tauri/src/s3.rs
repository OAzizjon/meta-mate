//! Spike S3 without a person at the desk: `--s3-selftest` opens our own fixture page
//! (`s3.html`, a login-like form) without taking focus, walks its UI Automation tree
//! from Rust and checks what the redaction relies on: the password field reports
//! IsPassword, the revealed password and the eye button are caught by their labels
//! (D37), the e-mail field is not, and a warm walk stays under 300 ms. Prints one JSON
//! line and exits 0 (pass) or 1. Only our own window is read: no user data.

use std::thread::sleep;
use std::time::{Duration, Instant};

use mm_platform::UiSnapshot;
use mm_win32::uia::{UiaWalker, WalkLimits};
use mm_win32::{HWND, focus, overlay};
use serde::Serialize;
use tauri::{AppHandle, WebviewUrl, WebviewWindowBuilder};

const WARM_LIMIT_MS: u128 = 300;

#[derive(Serialize, Default)]
struct Report {
    passed: bool,
    foreground_kept: bool,
    walks_ms: Vec<u128>,
    elements: usize,
    interactive: usize,
    truncated: bool,
    found_button: bool,
    password_is_password: bool,
    revealed_is_password_like: bool,
    reveal_button_is_password_like: bool,
    email_not_flagged: bool,
    error: Option<String>,
}

pub fn spawn(app: AppHandle, before: HWND) {
    let built = WebviewWindowBuilder::new(&app, "s3", WebviewUrl::App("s3.html".into()))
        .title("S3")
        .inner_size(480.0, 420.0)
        .visible(false)
        .focused(false)
        .focusable(false)
        .skip_taskbar(true)
        .build();
    let hwnd = built.as_ref().ok().and_then(|w| w.hwnd().ok());
    let Some(hwnd) = hwnd else {
        println!("{{\"passed\":false,\"error\":\"no fixture window\"}}");
        app.exit(1);
        return;
    };
    let shown = overlay::prepare(hwnd, overlay::OverlayKind::Robot).is_ok();
    overlay::show(hwnd);
    let (hwnd, before) = (hwnd.0 as isize, before.0 as isize);
    std::thread::spawn(move || {
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        let mut report = run(hwnd);
        report.foreground_kept = shown && focus::foreground().0 as isize == before;
        report.passed &= report.foreground_kept;
        println!("{}", serde_json::to_string(&report).unwrap_or_default());
        app.exit(if report.passed { 0 } else { 1 });
    });
}

fn run(hwnd: HWND) -> Report {
    let mut report = Report::default();
    let walker = match UiaWalker::new() {
        Ok(walker) => walker,
        Err(error) => {
            report.error = Some(error.to_string());
            return report;
        }
    };
    // Let the page load; Chromium builds its accessibility tree on the first query, so
    // retry a cold walk that sees too little (S3 checklist, Gemini #431).
    sleep(Duration::from_millis(1200));
    let mut snapshot = None;
    for _ in 0..4 {
        let started = Instant::now();
        let result = walker.snapshot(hwnd, WalkLimits::default());
        report.walks_ms.push(started.elapsed().as_millis());
        match result {
            Ok(s) => {
                let enough = interactive(&s) >= 6;
                snapshot = Some(s);
                if enough && report.walks_ms.len() >= 2 {
                    break;
                }
            }
            Err(error) => report.error = Some(error.to_string()),
        }
        sleep(Duration::from_millis(300));
    }
    let Some(snapshot) = snapshot else {
        return report;
    };
    let edit = |name: &str| {
        snapshot
            .elements
            .iter()
            .filter(|e| e.control_type == "Edit" && e.name.trim() == name)
            .collect::<Vec<_>>()
    };
    report.elements = snapshot.elements.len();
    report.interactive = interactive(&snapshot);
    report.truncated = snapshot.truncated;
    report.found_button = snapshot
        .elements
        .iter()
        .any(|e| e.control_type == "Button" && e.name == "Make text bigger" && e.rect.is_some());
    let passwords = edit("Password");
    report.password_is_password = passwords.iter().any(|e| e.is_password);
    report.revealed_is_password_like = passwords.iter().any(|e| !e.is_password && e.password_like);
    report.reveal_button_is_password_like = snapshot
        .elements
        .iter()
        .any(|e| e.control_type == "Button" && e.name == "Show password" && e.password_like);
    report.email_not_flagged = edit("Email")
        .iter()
        .any(|e| !e.password_like && !e.is_password);
    let warm = report.walks_ms.last().copied().unwrap_or(u128::MAX);
    report.passed = !report.truncated
        && report.found_button
        && report.password_is_password
        && report.revealed_is_password_like
        && report.reveal_button_is_password_like
        && report.email_not_flagged
        && warm < WARM_LIMIT_MS;
    report
}

fn interactive(snapshot: &UiSnapshot) -> usize {
    snapshot.elements.iter().filter(|e| e.interactive).count()
}
