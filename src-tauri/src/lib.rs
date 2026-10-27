//! MetaMate's Tauri shell: wiring only, no business logic (docs/project-structure.md).
//!
//! `--s1-selftest` runs spike S1 unattended (docs/architecture.md section 12): it shows
//! Mate and the highlight, moves Mate across every monitor, checks that the user's
//! foreground window never changed and that DPI follows each monitor, prints a JSON
//! report and exits with 0 (pass) or 1. `--voice-selftest FILE.wav` runs the voice path
//! against the dev proxy without any window (voice_selftest.rs). `--ptt-probe` prints
//! push-to-talk events for 60 s without any window (spike S2, ptt.rs).

#[allow(dead_code)] // wired to push-to-talk next (Day 2)
mod mic;
mod overlay;
mod proxy;
#[cfg(windows)]
mod ptt;
#[cfg(windows)]
mod s3;
#[cfg(windows)]
mod selftest;
mod voice_selftest;

/// Starts the app. Errors at startup are printed and end the process with code 1.
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = voice_selftest::run_if_requested(&args) {
        std::process::exit(code);
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

    let result = tauri::Builder::default()
        .setup(move |app| {
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
            let overlays = overlay::create(app.handle())?;
            #[cfg(windows)]
            if selftest {
                selftest::spawn(app.handle().clone(), overlays, before, per_monitor_v2);
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
