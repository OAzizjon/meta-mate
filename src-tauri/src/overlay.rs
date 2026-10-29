//! Mate's window and one highlight window per monitor: built hidden, made non-activating,
//! then shown without taking focus (invariant 1; CLAUDE.md "Known pitfalls": never show a
//! Tauri window before `focusable(false)`).

use mm_core::constants::APP_NAME;
use tauri::{AppHandle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// WebView2 arguments for every window of the app (they share one browser process, which
/// refuses different arguments). wry's defaults, plus autoplay: push-to-talk is not a DOM
/// gesture, yet Mate must speak (spike S7, docs/architecture.md section 7).
pub const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection      --autoplay-policy=no-user-gesture-required";

/// Mate's size in CSS pixels at 100 %; Windows scales it with the monitor.
pub const MATE_SIZE: f64 = 220.0;

#[cfg(windows)]
static HIGHLIGHTS_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(windows)]
pub(crate) fn highlights_ready() -> bool {
    HIGHLIGHTS_READY.load(std::sync::atomic::Ordering::Acquire)
}

pub struct Overlays {
    pub mate: WebviewWindow,
    /// One highlight window per monitor, in `mm_win32::monitors::monitors()` order:
    /// `highlight-0` covers the first monitor's area (highlight.rs). Monitors added, removed
    /// or rearranged while the app runs are not followed.
    #[cfg(windows)]
    pub highlights: Vec<(WebviewWindow, mm_win32::monitors::Monitor)>,
}

/// Builds and shows only Mate; monitor WebViews are deferred until setup returns.
pub fn create_mate(app: &AppHandle) -> Result<Overlays, Box<dyn std::error::Error>> {
    let mate = builder(app, "mate", "mate.html")
        .inner_size(MATE_SIZE, MATE_SIZE)
        .build()?;
    #[cfg(windows)]
    {
        use mm_win32::overlay::{self, OverlayKind};
        crate::voice::overlays_changed(&mate);
        let mate_hwnd = mate.hwnd()?;
        overlay::prepare(mate_hwnd, OverlayKind::Robot)?;
        let monitors = mm_win32::monitors::monitors();
        // Never show Mate at an arbitrary place: no monitor, no Mate.
        let area = starting_home(app, &monitors).ok_or("no monitor to place Mate on")?;
        overlay::place(mate_hwnd, area)?;
        overlay::place(mate_hwnd, area)?;
        overlay::show(mate_hwnd);
    }
    Ok(Overlays {
        mate,
        #[cfg(windows)]
        highlights: Vec::new(),
    })
}

/// Called off the event thread, after Mate is visible. Each window is prepared
/// before its first show; no Tauri show setter may rewrite its no-activate styles.
#[cfg(windows)]
pub fn create_highlights(
    app: &AppHandle,
) -> Result<Vec<(WebviewWindow, mm_win32::monitors::Monitor)>, Box<dyn std::error::Error>> {
    use mm_win32::overlay::{self, OverlayKind};
    let monitors = mm_win32::monitors::monitors();
    let mut highlights = Vec::with_capacity(monitors.len());
    for (index, monitor) in monitors.into_iter().enumerate() {
        let label = crate::highlight::label(index);
        let highlight = builder(app, &label, "highlight.html")
            .inner_size(400.0, 300.0)
            .build()?;
        crate::voice::overlays_changed(&highlight);
        // The highlight never takes a click: everything goes to the app underneath.
        highlight.set_ignore_cursor_events(true)?;
        let hwnd = highlight.hwnd()?;
        overlay::prepare(hwnd, OverlayKind::Highlight)?;
        // A scale change can preserve logical size; restore exact physical bounds.
        overlay::place(hwnd, monitor.area)?;
        overlay::place(hwnd, monitor.area)?;
        highlights.push((highlight, monitor));
    }
    HIGHLIGHTS_READY.store(true, std::sync::atomic::Ordering::Release);
    crate::voice::invalidate_overlays();
    Ok(highlights)
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
        .additional_browser_args(BROWSER_ARGS)
}

/// Mate's resting place: the bottom-right corner of the primary monitor's work area,
/// sized for that monitor's scale.
#[cfg(windows)]
pub fn home(monitors: &[mm_win32::monitors::Monitor]) -> Option<mm_core::geom::PhysRect> {
    let monitor = monitors.iter().find(|m| m.primary).or(monitors.first())?;
    corner(monitor)
}

/// Restores a saved home only when its monitor is still connected, at its current DPI.
#[cfg(windows)]
pub fn starting_home(
    app: &AppHandle,
    monitors: &[mm_win32::monitors::Monitor],
) -> Option<mm_core::geom::PhysRect> {
    use tauri::Manager;
    if let Some(saved) = app
        .state::<crate::settings::SettingsStore>()
        .get()
        .mate_home
    {
        let point = mm_core::geom::PhysPoint {
            x: saved.x,
            y: saved.y,
        };
        for monitor in monitors {
            let size = (MATE_SIZE * monitor.scale()).round() as i32;
            if let Some(rect) =
                mm_core::drag::restore(point, size, size, monitor.area, monitor.work_area)
            {
                return Some(rect);
            }
        }
    }
    home(monitors)
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
