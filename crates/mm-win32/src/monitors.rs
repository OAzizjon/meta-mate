//! The monitors of the virtual screen, in physical pixels with their DPI.

use mm_core::geom::PhysRect;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::core::BOOL;

/// One monitor: its full area, its work area (without the taskbar) and its effective DPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub area: PhysRect,
    pub work_area: PhysRect,
    pub dpi: u32,
    pub primary: bool,
}

impl Monitor {
    /// The scale factor Windows applies here (1.0 at 96 DPI, 1.5 at 144).
    pub fn scale(&self) -> f64 {
        f64::from(self.dpi) / 96.0
    }
}

/// Every monitor, in the order Windows reports them. Coordinates can be negative (a
/// monitor left of or above the primary one). Needs a Per-Monitor v2 aware process to
/// see real pixels.
pub fn monitors() -> Vec<Monitor> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: `collect` only runs during this call and receives `&mut handles` through
    // `LPARAM`, which outlives the call; no clip rectangle and no DC are passed.
    let _ = unsafe {
        EnumDisplayMonitors(
            None::<HDC>,
            None,
            Some(collect),
            LPARAM(&mut handles as *mut Vec<HMONITOR> as isize),
        )
    };
    handles.into_iter().filter_map(describe).collect()
}

unsafe extern "system" fn collect(
    monitor: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: `data` is the `&mut Vec<HMONITOR>` passed by `monitors`, alive for the
    // whole enumeration and not aliased elsewhere meanwhile.
    let handles = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
    handles.push(monitor);
    true.into()
}

fn describe(monitor: HMONITOR) -> Option<Monitor> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a properly sized, writable MONITORINFO on our stack.
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
    // SAFETY: both out pointers point at local u32s that live past the call.
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }.ok()?;
    Some(Monitor {
        area: rect(info.rcMonitor)?,
        work_area: rect(info.rcWork)?,
        dpi: dpi_x,
        primary: info.dwFlags & 1 == 1, // MONITORINFOF_PRIMARY
    })
}

fn rect(r: RECT) -> Option<PhysRect> {
    PhysRect::new(r.left, r.top, r.right, r.bottom).ok()
}
