//! The window a "where is it?" question is about: the user's foreground window, its
//! visible popups (menus and dropdowns are separate windows, Known pitfalls) and the screen
//! region that holds them, in physical pixels (invariant 2). Our own windows are never a
//! target and never one of its popups. Fails closed: a query that does not answer means no
//! target, never a guessed one (invariant 3).

use mm_core::geom::PhysRect;
use mm_platform::TargetWindow;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClassNameW,
    GetLayeredWindowAttributes, GetWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, LWA_ALPHA, WS_EX_LAYERED, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT,
};
use windows::core::{BOOL, PWSTR};

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

/// Known RDP, VMware, VirtualBox, TeamViewer, AnyDesk and Hyper-V viewer classes.
/// Generic UI toolkit classes are not evidence of a remote viewer.
fn remote_class(class: &str) -> bool {
    let class = class.to_ascii_lowercase();
    [
        "tscshellcontainerclass",
        "tscaxhostclass",
        "rail_window",
        "vmware",
        "vmuiview",
        "vmuiframe",
        "vmplayer",
        "virtualbox",
        "teamviewer",
        "tv_",
        "anydesk",
        "ad_salat",
        "vmconnect",
        "rustdesk",
        "parsec",
        "vncviewer",
        "turbovnc",
    ]
    .iter()
    .any(|prefix| class.starts_with(prefix))
}

fn remote_executable(path: &str) -> bool {
    let name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    [
        "mstsc.exe",
        "rdcman.exe",
        "mremoteng.exe",
        "msrdc.exe",
        "vmconnect.exe",
        "virtualbox.exe",
        "virtualboxvm.exe",
        "vmware.exe",
        "vmplayer.exe",
        "vmware-view.exe",
        "teamviewer.exe",
        "teamviewer_desktop.exe",
        "anydesk.exe",
        "rustdesk.exe",
        "parsec.exe",
        "parsecd.exe",
        "vncviewer.exe",
        "tvnviewer.exe",
        "wfica32.exe",
    ]
    .contains(&name.as_str())
}

/// Class names vary by release and UI toolkit; also check the owning executable.
/// An inaccessible process cannot establish that this is a safe local target.
fn local_process(process_id: u32) -> Option<bool> {
    Some(!remote_executable(&process_path(process_id)?))
}

fn process_path(process_id: u32) -> Option<String> {
    // SAFETY: a read-only query handle; no memory in the other process is read.
    let Ok(process) =
        (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) })
    else {
        return None;
    };
    let mut buffer = [0u16; 32768];
    let mut len = buffer.len() as u32;
    // SAFETY: the buffer is writable for `len` UTF-16 units; the handle stays open.
    let read = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    };
    // SAFETY: this is the handle acquired above, closed once after its last query.
    let _ = unsafe { CloseHandle(process) };
    if read.is_err() || len == 0 {
        return None;
    }
    let path = buffer.get(..len as usize)?;
    Some(String::from_utf16_lossy(path))
}

/// The app's name as its executable's file name without ".exe" ("SystemSettings",
/// "msedge"), never a window title (titles can hold private text). None when the process
/// cannot be read.
pub fn process_name(process_id: u32) -> Option<String> {
    executable_stem(&process_path(process_id)?).map(str::to_owned)
}

fn executable_stem(path: &str) -> Option<&str> {
    let file = path.rsplit(['\\', '/']).next()?;
    let end = file.len().saturating_sub(4);
    let stem = match file.get(end..) {
        Some(extension) if extension.eq_ignore_ascii_case(".exe") => &file[..end],
        _ => file,
    };
    (!stem.trim().is_empty()).then_some(stem)
}

fn settings_executable(path: &str) -> bool {
    path.rsplit(['\\', '/'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("SystemSettings.exe"))
}

/// True only when the remembered window is still foreground and belongs to Settings.
/// Windows 10 can host Settings inside an ApplicationFrameHost window.
pub fn settings_foreground(window: TargetWindow) -> bool {
    let hwnd = HWND(window.handle as usize as *mut std::ffi::c_void);
    if crate::focus::foreground() != hwnd || remembered_target(window, &[]).is_none() {
        return false;
    }
    if process_path(window.process_id).is_some_and(|path| settings_executable(&path)) {
        return true;
    }
    unsafe extern "system" fn child(hwnd: HWND, data: LPARAM) -> BOOL {
        let mut pid = 0;
        // SAFETY: pid is writable; data points to the bool alive during EnumChildWindows.
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if process_path(pid).is_some_and(|path| settings_executable(&path)) {
                *(data.0 as *mut bool) = true;
            }
        }
        BOOL(1)
    }
    let mut found = false;
    // SAFETY: the callback is synchronous and found outlives the enumeration.
    let _ = unsafe {
        EnumChildWindows(
            Some(hwnd),
            Some(child),
            LPARAM(&mut found as *mut bool as isize),
        )
    };
    found
}

/// The user's window, its popups and the region to capture. Debug carries handles and
/// rectangles only, never a title (convention 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Remote viewers need the person's permission for each capture.
    pub requires_consent: bool,
    pub window: TargetWindow,
    pub hwnd: HWND,
    /// The visible frame without the drop shadow (`DWMWA_EXTENDED_FRAME_BOUNDS`).
    pub frame: PhysRect,
    /// Visible windows whose owner chain reaches it, plus its active unowned menus.
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
    let local = local_process(process_id)?;
    let requires_consent = !local || remote_class(&class);
    let frame = frame_of(hwnd)?;
    let (popup_hwnds, popups): (Vec<HWND>, Vec<PhysRect>) =
        popups_of(hwnd, thread_id, own).into_iter().unzip();
    let region = region_of(frame, &popups, monitor_of(frame)?)?;
    Some(Target {
        requires_consent,
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
    if style & WS_EX_LAYERED.0 == 0 {
        return false;
    }
    let mut alpha = 255u8;
    let mut flags = LWA_ALPHA;
    // SAFETY: both output pointers are writable locals that outlive this call.
    let read =
        unsafe { GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), Some(&mut flags)) };
    transparent_attributes(
        style & WS_EX_LAYERED.0 != 0,
        read.is_ok().then_some((flags.0 & LWA_ALPHA.0 != 0, alpha)),
    ) || per_pixel_overlay(
        style & WS_EX_LAYERED.0 != 0 && read.is_err(),
        style & WS_EX_TOPMOST.0 != 0,
        style & WS_EX_TRANSPARENT.0 != 0,
        frame_of(hwnd).and_then(|frame| Some((frame, monitor_of(frame)?))),
    )
}

/// Per-pixel overlay tools have no constant-alpha attributes. Only exempt a
/// topmost, click-through surface that covers at least 90 percent of its monitor.
fn per_pixel_overlay(
    per_pixel: bool,
    topmost: bool,
    click_through: bool,
    bounds: Option<(PhysRect, PhysRect)>,
) -> bool {
    if !per_pixel || !topmost || !click_through {
        return false;
    }
    let Some((frame, monitor)) = bounds else {
        return false;
    };
    let Some(covered) = clip_to_region(frame, monitor) else {
        return false;
    };
    let area = |r: PhysRect| {
        (i128::from(r.right) - i128::from(r.left)) * (i128::from(r.bottom) - i128::from(r.top))
    };
    area(covered) * 10 >= area(monitor) * 9
}

fn transparent_attributes(layered: bool, attributes: Option<(bool, u8)>) -> bool {
    layered && attributes == Some((true, 0))
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
                && class_name(window).is_some_and(|class| !remote_class(&class))
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

/// Ownership must reach the target. Unowned native menus additionally need its
/// foreground and thread; a shared UI thread alone proves no ownership.
fn belongs_to(window: HWND, owner: HWND, thread_id: u32) -> bool {
    let (reaches_target, unowned) = owner_chain(window, owner, |next| {
        // SAFETY: plain handle query; missing or stale owners terminate the chain.
        unsafe { GetWindow(next, GW_OWNER) }
            .ok()
            .filter(|found| !found.is_invalid())
    });
    // SAFETY: no out pointer; a stale handle gives thread 0, never the target's.
    popup_owned(
        reaches_target,
        unowned,
        class_name(window).as_deref() == Some(MENU_CLASS),
        crate::focus::foreground() == owner,
        unsafe { GetWindowThreadProcessId(window, None) } == thread_id,
    )
}

fn owner_chain(
    window: HWND,
    target: HWND,
    mut owner_of: impl FnMut(HWND) -> Option<HWND>,
) -> (bool, bool) {
    let mut chain = Vec::new();
    let mut next = window;
    for _ in 0..64 {
        let Some(found) = owner_of(next) else {
            return (false, chain.is_empty());
        };
        if found == target {
            return (true, false);
        }
        if chain.contains(&found) {
            return (false, false);
        }
        chain.push(found);
        next = found;
    }
    (false, false)
}

/// A follow-up may enter only a newly opened owned dialog in the same process.
pub fn lesson_followup(
    lesson: TargetWindow,
    candidate: TargetWindow,
    existing: Option<&[u64]>,
) -> bool {
    let handle = |n: u64| HWND(n as usize as *mut std::ffi::c_void);
    let owned = candidate != lesson
        && owner_chain(handle(candidate.handle), handle(lesson.handle), |next| {
            // SAFETY: a read-only owner query; stale handles end the chain.
            unsafe { GetWindow(next, GW_OWNER) }
                .ok()
                .filter(|found| !found.is_invalid())
        })
        .0;
    followup_allowed(
        lesson,
        candidate,
        existing.is_none_or(|handles| handles.contains(&candidate.handle)),
        owned,
    )
}

fn followup_allowed(
    lesson: TargetWindow,
    candidate: TargetWindow,
    existed: bool,
    owned: bool,
) -> bool {
    lesson == candidate || (lesson.process_id == candidate.process_id && !existed && owned)
}

/// Window handles present before a lesson step, used to distinguish a new dialog.
pub fn existing_windows() -> Option<Vec<u64>> {
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: synchronous enumeration with an exclusive stack-local vector.
    unsafe {
        EnumWindows(
            Some(collect),
            LPARAM(std::ptr::from_mut(&mut handles) as isize),
        )
    }
    .ok()?;
    Some(handles.into_iter().map(|h| h.0 as usize as u64).collect())
}

fn popup_owned(
    reaches_target: bool,
    unowned: bool,
    menu: bool,
    active: bool,
    same_thread: bool,
) -> bool {
    reaches_target || (unowned && menu && active && same_thread)
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
    fn settings_identity_uses_the_executable_basename() {
        for path in [
            "C:\\Windows\\ImmersiveControlPanel\\SystemSettings.exe",
            "SYSTEMSETTINGS.EXE",
        ] {
            assert!(settings_executable(path));
        }
        for path in [
            "",
            "explorer.exe",
            "ApplicationFrameHost.exe",
            "FakeSystemSettings.exe",
            "SystemSettings.exe.bak",
        ] {
            assert!(!settings_executable(path));
        }
    }

    #[test]
    fn the_app_name_is_the_executable_without_its_extension() {
        assert_eq!(
            executable_stem("C:\\Windows\\ImmersiveControlPanel\\SystemSettings.exe"),
            Some("SystemSettings")
        );
        assert_eq!(executable_stem("C:/Apps/Telegram.EXE"), Some("Telegram"));
        assert_eq!(executable_stem("C:\\Tools\\viewer"), Some("viewer"));
        assert_eq!(executable_stem("C:\\Программы\\Почта.exe"), Some("Почта"));
        assert_eq!(executable_stem("exe"), Some("exe"));
        for path in ["", "C:\\Apps\\", "C:\\Apps\\.exe"] {
            assert_eq!(executable_stem(path), None, "{path}");
        }
    }

    #[test]
    fn demo_apps_are_local_but_identity_does_not_certify_their_tree() {
        for name in [
            "C:\\Apps\\Telegram.exe",
            "NOTEPAD.EXE",
            "SystemSettings.exe",
        ] {
            assert!(!remote_executable(name), "{name}");
        }
        for name in [
            "msedge.exe",
            "RDCMan.exe",
            "mRemoteNG.exe",
            "viewer.exe",
            "notepad.exe.bak",
        ] {
            assert_eq!(
                remote_executable(name),
                matches!(name, "RDCMan.exe" | "mRemoteNG.exe"),
                "{name}"
            );
        }
    }

    #[test]
    fn lesson_follows_only_its_window_or_a_new_owned_dialog() {
        let lesson = TargetWindow {
            handle: 1,
            process_id: 10,
        };
        let other = TargetWindow {
            handle: 2,
            process_id: 10,
        };
        assert!(followup_allowed(lesson, lesson, true, false));
        // Includes separate browser windows and shared ApplicationFrameHost windows.
        assert!(!followup_allowed(lesson, other, false, false));
        assert!(!followup_allowed(lesson, other, true, true));
        assert!(followup_allowed(lesson, other, false, true));
        assert!(!followup_allowed(
            lesson,
            TargetWindow {
                process_id: 11,
                ..other
            },
            false,
            true
        ));
    }

    #[test]
    fn click_through_is_not_visual_transparency() {
        assert!(!transparent_attributes(false, Some((true, 0))));
        assert!(!transparent_attributes(true, None));
        assert!(!transparent_attributes(true, Some((false, 0))));
        assert!(!transparent_attributes(true, Some((true, 1))));
        assert!(transparent_attributes(true, Some((true, 0))));
    }

    #[test]
    fn popup_requires_ownership_or_an_active_unowned_menu() {
        assert!(popup_owned(true, false, false, false, false));
        assert!(!popup_owned(false, false, true, true, true));
        assert!(!popup_owned(false, true, false, true, true));
        assert!(!popup_owned(false, true, true, false, true));
        assert!(!popup_owned(false, true, true, true, false));
        assert!(popup_owned(false, true, true, true, true));
    }

    #[test]
    fn owner_chain_follows_nested_owners_and_refuses_cycles() {
        let handle = |n: usize| HWND(n as *mut std::ffi::c_void);
        let (child, middle, target, other) = (handle(1), handle(2), handle(3), handle(4));
        assert_eq!(
            owner_chain(child, target, |w| {
                if w == child {
                    Some(middle)
                } else if w == middle {
                    Some(target)
                } else {
                    None
                }
            }),
            (true, false)
        );
        assert_eq!(owner_chain(child, target, |_| None), (false, true));
        assert_eq!(
            owner_chain(child, target, |w| (w == child).then_some(other)),
            (false, false)
        );
        assert_eq!(owner_chain(child, target, |_| Some(middle)), (false, false));
    }

    #[test]
    fn remote_viewers_require_consent_without_blocking_local_toolkits() {
        for class in [
            "TscShellContainerClass",
            "TscAxHostClass",
            "VMUIFrame",
            "VMwareUnityHostWndClass",
            "TeamViewer_RemoteScreen",
            "TV_RemoteDesktop",
            "ad_salat",
            "AnyDesk",
            "vmconnect",
        ] {
            assert!(eligible_class(class), "{class}");
            assert!(remote_class(class), "{class}");
        }
        for name in [
            "mstsc.exe",
            "RDCMan.exe",
            "mRemoteNG.exe",
            "VMConnect.exe",
            "VirtualBoxVM.exe",
            "vmware.exe",
            "vmplayer.exe",
            "TeamViewer.exe",
            "AnyDesk.exe",
            "RustDesk.exe",
            "parsecd.exe",
        ] {
            assert!(remote_executable(&format!("C:\\Apps\\{name}")));
        }
        assert!(!remote_executable("C:\\Apps\\notepad.exe"));
        for name in ["SystemSettings.exe", "msedge.exe", "Telegram.exe"] {
            assert!(!remote_executable(name), "{name}");
        }
        for class in [
            "Qt5QWindowIcon",
            "Qt6QWindowIcon",
            "WindowsForms10.Window.8.app.0.abc",
        ] {
            assert!(eligible_class(class));
        }
    }

    #[test]
    fn per_pixel_overlay_requires_all_evidence_and_monitor_coverage() {
        let bounds = Some((rect(0, 0, 1920, 972), MONITOR));
        assert!(per_pixel_overlay(true, true, true, bounds));
        assert!(!per_pixel_overlay(false, true, true, bounds));
        assert!(!per_pixel_overlay(true, false, true, bounds));
        assert!(!per_pixel_overlay(true, true, false, bounds));
        assert!(!per_pixel_overlay(true, true, true, None));
        assert!(!per_pixel_overlay(
            true,
            true,
            true,
            Some((rect(0, 0, 1920, 971), MONITOR))
        ));
        assert!(!per_pixel_overlay(
            true,
            true,
            true,
            Some((rect(1800, 0, 3720, 1080), MONITOR))
        ));
    }

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
