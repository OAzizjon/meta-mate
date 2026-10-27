//! Mate's window and the highlight window: built hidden, made non-activating, then
//! shown without taking focus (invariant 1; CLAUDE.md "Known pitfalls": never show a
//! Tauri window before `focusable(false)`).

use mm_core::constants::APP_NAME;
use tauri::{AppHandle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// Mate's size in CSS pixels at 100 %; Windows scales it with the monitor.
pub const MATE_SIZE: f64 = 220.0;

pub struct Overlays {
    pub mate: WebviewWindow,
    pub highlight: WebviewWindow,
}

pub fn create(app: &AppHandle) -> Result<Overlays, Box<dyn std::error::Error>> {
    let mate = builder(app, "mate", "mate.html")
        .inner_size(MATE_SIZE, MATE_SIZE)
        .build()?;
    let highlight = builder(app, "highlight", "highlight.html")
        .inner_size(400.0, 300.0)
        .build()?;
    // The highlight never takes a click: everything goes to the app underneath.
    highlight.set_ignore_cursor_events(true)?;

    #[cfg(windows)]
    {
        use mm_win32::overlay::{self, OverlayKind};
        let mate_hwnd = mate.hwnd()?;
        let highlight_hwnd = highlight.hwnd()?;
        overlay::prepare(mate_hwnd, OverlayKind::Robot)?;
        overlay::prepare(highlight_hwnd, OverlayKind::Highlight)?;
        let monitors = mm_win32::monitors::monitors();
        // Never show Mate at an arbitrary place: no monitor, no Mate.
        let area = home(&monitors).ok_or("no monitor to place Mate on")?;
        overlay::place(mate_hwnd, area)?;
        // The highlight covers the primary monitor; one per monitor comes with Day 4.
        if let Some(primary) = monitors.iter().find(|m| m.primary).or(monitors.first()) {
            overlay::place(highlight_hwnd, primary.area)?;
        }
        overlay::show(mate_hwnd);
    }
    Ok(Overlays { mate, highlight })
}

/// Hidden, unfocusable, transparent, undecorated, topmost, not in the taskbar.
fn builder<'a>(
    app: &'a AppHandle,
    label: &'a str,
    page: &str,
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(page.into()))
        .title(APP_NAME)
        .visible(false)
        .focused(false)
        .focusable(false)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
}

/// Mate's resting place: the bottom-right corner of the primary monitor's work area,
/// sized for that monitor's scale.
#[cfg(windows)]
pub fn home(monitors: &[mm_win32::monitors::Monitor]) -> Option<mm_core::geom::PhysRect> {
    let monitor = monitors.iter().find(|m| m.primary).or(monitors.first())?;
    corner(monitor)
}

/// The bottom-right corner of `monitor`'s work area, 24 px in, Mate-sized.
#[cfg(windows)]
pub fn corner(monitor: &mm_win32::monitors::Monitor) -> Option<mm_core::geom::PhysRect> {
    let size = (MATE_SIZE * monitor.scale()).round() as i32;
    let margin = (24.0 * monitor.scale()).round() as i32;
    let work = monitor.work_area;
    mm_core::geom::PhysRect::new(
        work.right - margin - size,
        work.bottom - margin - size,
        work.right - margin,
        work.bottom - margin,
    )
    .ok()
}
