//! The window a "where is it?" question is about: the user's foreground window, its
//! visible popups (menus and dropdowns are separate windows, Known pitfalls) and the screen
//! region that holds them, in physical pixels (invariant 2). Our own windows are never a
//! target and never one of its popups. Fails closed: a query that does not answer means no
//! target, never a guessed one (invariant 3).

use mm_core::geom::PhysRect;
use mm_platform::TargetWindow;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetLayeredWindowAttributes,
    GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, LWA_ALPHA, WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::BOOL;

/// Window classes of the desktop and the taskbars: never a target.
const SHELL_CLASSES: [&str; 6] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow",
    "TopLevelWindowForOverflowXamlIsland",
];

/// The class of menu windows (context menus, menu bar drop-downs).
const MENU_CLASS: &str = "#32768";

fn eligible_class(class: &str) -> bool {
    !SHELL_CLASSES.contains(&class) && class != MENU_CLASS
}

/// The user's window, its popups and the region to capture. Debug carries handles and
/// rectangles only, never a title (convention 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub window: TargetWindow,
    pub hwnd: HWND,
    /// The visible frame without the drop shadow (`DWMWA_EXTENDED_FRAME_BOUNDS`).
    pub frame: PhysRect,
    /// Visible windows that belong to it: owned ones, and menus or dropdowns of its thread.
    pub popups: Vec<PhysRect>,
    /// The same popups' handles, in the same order (UIA walks each one).
    pub popup_hwnds: Vec<HWND>,
    /// What to capture: frame and popups together, clipped to the frame's monitor.
    pub region: PhysRect,
}

/// The foreground window as a target, or `None` when there is nothing to point at: one of
/// `own` (Mate, the highlight), the desktop or a taskbar, a hidden, minimized, cloaked
/// (another virtual desktop, a suspended app) or zero-sized window.
pub fn foreground_target(own: &[HWND]) -> Option<Target> {
    let hwnd = crate::focus::foreground();
    target_of(hwnd, own)
}

/// Rebuild a remembered target without activating it; recycled handles fail closed.
pub fn remembered_target(window: TargetWindow, own: &[HWND]) -> Option<Target> {
    let target = target_of(HWND(window.handle as usize as *mut std::ffi::c_void), own)?;
    (target.window == window).then_some(target)
}

fn target_of(hwnd: HWND, own: &[HWND]) -> Option<Target> {
    if hwnd.is_invalid() || own.contains(&hwnd) {
        return None;
    }
    let class = class_name(hwnd)?;
    if !eligible_class(&class) || !shown(hwnd) {
        return None;
    }
    let mut process_id = 0u32;
    // SAFETY: the out pointer is a local u32 that outlives the call.
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    // Never one of our own windows, even one `own` does not list.
    if thread_id == 0 || process_id == std::process::id() {
        return None;
    }
    let frame = frame_of(hwnd)?;
    let (popup_hwnds, popups): (Vec<HWND>, Vec<PhysRect>) =
        popups_of(hwnd, thread_id, own).into_iter().unzip();
    let region = region_of(frame, &popups, monitor_of(frame)?)?;
    Some(Target {
        window: TargetWindow {
            handle: hwnd.0 as usize as u64,
            process_id,
        },
        hwnd,
        frame,
        popups,
        popup_hwnds,
        region,
    })
}

/// Whether the window a look was taken of is still there, shown and exactly where it was:
/// pointing at a stale picture would ring whatever is there now (invariant 5).
pub fn still_shown(window: TargetWindow, frame: PhysRect) -> bool {
    let hwnd = HWND(window.handle as usize as *mut std::ffi::c_void);
    let mut process_id = 0u32;
    // SAFETY: the out pointer is a local u32; a stale handle gives thread 0.
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    thread != 0 && process_id == window.process_id && shown(hwnd) && frame_of(hwnd) == Some(frame)
}

/// Frames of other shown windows above the target, clipped to its capture region.
/// Topmost windows count too; uncertain transparency is included conservatively.
pub fn occluders(target: &Target, own: &[HWND]) -> Vec<PhysRect> {
    let mut above = AboveTarget {
        target: target.hwnd,
        handles: Vec::new(),
    };
    // SAFETY: the callback receives our exclusive stack-local `above`, which outlives
    // this synchronous enumeration. Stopping at the target intentionally returns false.
    let _ = unsafe {
        EnumWindows(
            Some(collect_above),
            LPARAM(std::ptr::from_mut(&mut above) as isize),
        )
    };
    above
        .handles
        .into_iter()
        .filter(|window| !target.popup_hwnds.contains(window) && !own.contains(window))
        .filter(|&window| shown(window) && !transparent(window))
        .filter_map(|window| clip_to_region(frame_of(window)?, target.region))
        .collect()
}

struct AboveTarget {
    target: HWND,
    handles: Vec<HWND>,
}

unsafe extern "system" fn collect_above(window: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `data` points to the exclusive `AboveTarget` supplied by `occluders`,
    // alive throughout this synchronous enumeration.
    let above = unsafe { &mut *(data.0 as *mut AboveTarget) };
    // EnumWindows visits top-level windows from top to bottom. Never collect the
    // target or continue into the windows underneath it.
    if window == above.target {
        return false.into();
    }
    above.handles.push(window);
    true.into()
}

/// Only skip transparency that can be established by cheap window attributes.
fn transparent(hwnd: HWND) -> bool {
    // SAFETY: reads a window long; a stale handle returns zero.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    if style & WS_EX_TRANSPARENT.0 != 0 {
        return true;
    }
    if style & WS_EX_LAYERED.0 == 0 {
        return false;
    }
    let mut alpha = 255u8;
    let mut flags = LWA_ALPHA;
    // SAFETY: both output pointers are writable locals that outlive this call.
    let read =
        unsafe { GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), Some(&mut flags)) };
    read.is_ok() && flags.0 & LWA_ALPHA.0 != 0 && alpha == 0
}

/// Half-open intersection: touching edges, empty or inverted rectangles add no pixels.
fn clip_to_region(frame: PhysRect, region: PhysRect) -> Option<PhysRect> {
    PhysRect::new(
        frame.left.max(region.left),
        frame.top.max(region.top),
        frame.right.min(region.right),
        frame.bottom.min(region.bottom),
    )
    .ok()
}

/// The capture region: the bounding box of `frame` and every popup, each clipped to
/// `monitor` first, so a popup on another monitor adds nothing. Empty popups are skipped;
/// an empty monitor, or a frame with nothing on the monitor, gives `None`.
pub fn region_of(frame: PhysRect, popups: &[PhysRect], monitor: PhysRect) -> Option<PhysRect> {
    // Revalidate: the fields are public, so a struct literal can be empty or inverted.
    let monitor = PhysRect::new(monitor.left, monitor.top, monitor.right, monitor.bottom).ok()?;
    let clip = |r: PhysRect| {
        PhysRect::new(
            r.left.max(monitor.left),
            r.top.max(monitor.top),
            r.right.min(monitor.right),
            r.bottom.min(monitor.bottom),
        )
        .ok()
    };
    let frame = clip(frame)?;
    Some(
        popups
            .iter()
            .filter_map(|&popup| clip(popup))
            .fold(frame, |bounds, popup| PhysRect {
                left: bounds.left.min(popup.left),
                top: bounds.top.min(popup.top),
                right: bounds.right.max(popup.right),
                bottom: bounds.bottom.max(popup.bottom),
            }),
    )
}

/// Visible, not minimized and not cloaked. A cloak query that fails counts as cloaked.
fn shown(hwnd: HWND) -> bool {
    // SAFETY: plain handle queries; a stale handle reads as not visible.
    let visible = unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() };
    let mut cloaked = 0u32;
    // SAFETY: `cloaked` is a writable u32 on our stack and its size is passed with it.
    let read = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            std::ptr::from_mut(&mut cloaked).cast(),
            size_of::<u32>() as u32,
        )
    };
    visible && read.is_ok() && cloaked == 0
}

/// The window's visible frame in physical pixels: `DWMWA_EXTENDED_FRAME_BOUNDS` (no drop
/// shadow), or the window rectangle when DWM does not answer; `None` when zero-sized.
fn frame_of(hwnd: HWND) -> Option<PhysRect> {
    let mut r = RECT::default();
    // SAFETY: `r` is a writable RECT on our stack and its size is passed with it.
    let dwm = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            std::ptr::from_mut(&mut r).cast(),
            size_of::<RECT>() as u32,
        )
    };
    if dwm.is_err() {
        // SAFETY: `r` is a writable RECT on our stack.
        unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
    }
    PhysRect::new(r.left, r.top, r.right, r.bottom).ok()
}

/// The full area of the monitor that holds the frame's center (the nearest one when the
/// center is on none).
fn monitor_of(frame: PhysRect) -> Option<PhysRect> {
    let center = frame.center();
    let probe = RECT {
        left: center.x,
        top: center.y,
        right: center.x.saturating_add(1),
        bottom: center.y.saturating_add(1),
    };
    // SAFETY: `probe` is a RECT on our stack, only read during the call.
    let monitor = unsafe { MonitorFromRect(&probe, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a properly sized, writable MONITORINFO on our stack.
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    let r = info.rcMonitor;
    PhysRect::new(r.left, r.top, r.right, r.bottom).ok()
}

/// The shown top-level windows that belong to `hwnd` with their frames, `own` excluded.
fn popups_of(hwnd: HWND, thread_id: u32, own: &[HWND]) -> Vec<(HWND, PhysRect)> {
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: `collect` only runs during this call and receives `&mut handles` through
    // `LPARAM`, which outlives the call. The result only says whether every window was seen.
    let _ = unsafe {
        EnumWindows(
            Some(collect),
            LPARAM(&mut handles as *mut Vec<HWND> as isize),
        )
    };
    handles
        .into_iter()
        .filter(|&window| {
            window != hwnd
                && !own.contains(&window)
                && belongs_to(window, hwnd, thread_id)
                && shown(window)
        })
        .filter_map(|window| Some((window, frame_of(window)?)))
        .collect()
}

unsafe extern "system" fn collect(window: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the `&mut Vec<HWND>` passed by `popups_of`, alive for the whole
    // enumeration and not aliased elsewhere meanwhile.
    let handles = unsafe { &mut *(data.0 as *mut Vec<HWND>) };
    handles.push(window);
    true.into()
}

/// Owned by `owner`, or a popup-style window of the same thread (a menu, a dropdown).
fn belongs_to(window: HWND, owner: HWND, thread_id: u32) -> bool {
    // SAFETY: plain handle and command; a window without an owner is an error, not a handle.
    if unsafe { GetWindow(window, GW_OWNER) }.is_ok_and(|found| found == owner) {
        return true;
    }
    // SAFETY: no out pointer; a stale handle gives thread 0, never the target's.
    if unsafe { GetWindowThreadProcessId(window, None) } != thread_id {
        return false;
    }
    // SAFETY: reads a window long; an invalid handle returns 0.
    let style = unsafe { GetWindowLongPtrW(window, GWL_STYLE) } as u32;
    (style & WS_POPUP.0) != 0 || class_name(window).as_deref() == Some(MENU_CLASS)
}

/// The window's class name; `None` when the window is gone.
fn class_name(hwnd: HWND) -> Option<String> {
    let mut buffer = [0u16; 256];
    // SAFETY: the buffer is a writable local array; the call writes at most its length.
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0)?;
    Some(String::from_utf16_lossy(buffer.get(..len)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_tray_and_menu_windows_never_replace_the_target() {
        for class in SHELL_CLASSES.into_iter().chain([MENU_CLASS]) {
            assert!(!eligible_class(class));
        }
        assert!(eligible_class("Chrome_WidgetWin_1"));
        assert!(eligible_class("Notepad"));
    }

    const MONITOR: PhysRect = PhysRect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PhysRect {
        PhysRect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn occluder_frames_are_clipped_on_every_side() {
        let region = rect(100, 200, 800, 700);
        assert_eq!(
            clip_to_region(rect(50, 150, 900, 750), region),
            Some(region)
        );
        assert_eq!(
            clip_to_region(rect(50, 300, 400, 750), region),
            Some(rect(100, 300, 400, 700))
        );
        assert_eq!(
            clip_to_region(rect(400, 150, 900, 500), region),
            Some(rect(400, 200, 800, 500))
        );
        let contained = rect(200, 300, 600, 500);
        assert_eq!(clip_to_region(contained, region), Some(contained));
    }

    #[test]
    fn occluder_edges_and_corners_do_not_intersect() {
        let region = rect(100, 200, 800, 700);
        for frame in [
            rect(0, 300, 100, 500),
            rect(800, 300, 900, 500),
            rect(200, 100, 600, 200),
            rect(200, 700, 600, 800),
            rect(800, 700, 900, 800),
            rect(900, 800, 1000, 900),
        ] {
            assert_eq!(clip_to_region(frame, region), None);
            assert_eq!(clip_to_region(region, frame), None);
        }
    }

    #[test]
    fn occluder_clipping_handles_negative_and_extreme_coordinates() {
        assert_eq!(
            clip_to_region(rect(-2000, -500, -500, 500), rect(-1920, -200, 0, 880)),
            Some(rect(-1920, -200, -500, 500))
        );
        assert_eq!(
            clip_to_region(rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX), MONITOR),
            Some(MONITOR)
        );
    }

    #[test]
    fn occluder_clipping_refuses_invalid_rectangles() {
        for invalid in [
            rect(20, 20, 20, 40),
            rect(20, 20, 40, 20),
            rect(40, 40, 20, 20),
        ] {
            assert_eq!(clip_to_region(invalid, MONITOR), None);
            assert_eq!(clip_to_region(MONITOR, invalid), None);
        }
    }

    #[test]
    fn the_frame_alone_is_the_region() {
        let frame = rect(100, 100, 900, 700);
        assert_eq!(region_of(frame, &[], MONITOR), Some(frame));
    }

    #[test]
    fn popups_extend_the_region() {
        let frame = rect(100, 100, 900, 700);
        let dropdown = rect(300, 650, 500, 950);
        let submenu = rect(880, 200, 1100, 400);
        assert_eq!(
            region_of(frame, &[dropdown, submenu], MONITOR),
            Some(rect(100, 100, 1100, 950))
        );
    }

    #[test]
    fn the_region_is_clipped_to_the_monitor() {
        let frame = rect(-50, -20, 800, 600);
        let menu = rect(700, 500, 1000, 1300);
        assert_eq!(
            region_of(frame, &[menu], MONITOR),
            Some(rect(0, 0, 1000, 1080))
        );
    }

    #[test]
    fn negative_coordinates_work() {
        let left_monitor = rect(-1920, -200, 0, 880);
        let frame = rect(-1500, 0, -300, 700);
        let popup = rect(-400, 600, 200, 900);
        assert_eq!(
            region_of(frame, &[popup], left_monitor),
            Some(rect(-1500, 0, 0, 880))
        );
    }

    #[test]
    fn nothing_on_the_monitor_is_none() {
        let frame = rect(2000, 100, 2800, 700);
        assert_eq!(region_of(frame, &[], MONITOR), None);
        // Half-open: touching the right edge is not overlapping it.
        let touching = rect(1920, 100, 2400, 700);
        assert_eq!(region_of(touching, &[], MONITOR), None);
    }

    #[test]
    fn empty_rectangles_are_skipped_or_refused() {
        let frame = rect(100, 100, 900, 700);
        let empty = rect(1000, 1000, 1000, 1050);
        let inverted = rect(50, 50, 10, 10);
        assert_eq!(region_of(frame, &[empty, inverted], MONITOR), Some(frame));
        assert_eq!(region_of(empty, &[frame], MONITOR), None);
        assert_eq!(region_of(frame, &[], rect(0, 0, 0, 1080)), None);
    }

    #[test]
    fn a_popup_on_another_monitor_adds_nothing() {
        let frame = rect(100, 100, 900, 700);
        let elsewhere = rect(2000, 100, 2300, 400);
        assert_eq!(region_of(frame, &[elsewhere], MONITOR), Some(frame));
    }

    #[test]
    fn a_popup_straddling_the_edge_adds_its_visible_part() {
        let frame = rect(1000, 100, 1800, 700);
        let menu = rect(1700, 300, 2100, 500);
        assert_eq!(
            region_of(frame, &[menu], MONITOR),
            Some(rect(1000, 100, 1920, 700))
        );
    }
}
