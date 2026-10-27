//! Per-Monitor DPI v2 (invariant 2).

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
    GetThreadDpiAwarenessContext, SetProcessDpiAwarenessContext,
};

/// Makes the process Per-Monitor DPI v2 aware and says whether it now is. Call it before
/// any window exists: WebView2 takes the awareness when it is created. A failure is
/// expected when the manifest already set it, so only the resulting state counts.
pub fn ensure_per_monitor_v2() -> bool {
    // SAFETY: takes a constant context and no pointers; failing changes nothing.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    is_per_monitor_v2()
}

/// Whether the calling thread runs Per-Monitor DPI v2 aware (v1 is not enough: v2 also
/// scales the non-client area and child windows).
pub fn is_per_monitor_v2() -> bool {
    // SAFETY: both functions take and return plain context handles, no pointers.
    unsafe {
        AreDpiAwarenessContextsEqual(
            GetThreadDpiAwarenessContext(),
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
        .as_bool()
    }
}

/// The DPI of the monitor a window is on (96 = 100 %, 144 = 150 %); 0 for an invalid
/// window.
pub fn dpi_for_window(hwnd: HWND) -> u32 {
    // SAFETY: an invalid handle makes the call return 0; nothing is dereferenced here.
    unsafe { GetDpiForWindow(hwnd) }
}
