//! The user's foreground window: the one our windows must never take over (invariant 1).

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

/// Brings one of our normal windows to the front, only right after the person asked for it
/// (a tray or menu click). No injected keys: Windows may refuse and just flash the taskbar
/// button, which is fine. Never for Mate, the highlight or anything an answer opens.
pub fn activate_on_request(hwnd: HWND) -> bool {
    // SAFETY: a window handle owned by this process; the call changes nothing else.
    unsafe { SetForegroundWindow(hwnd).as_bool() }
}

/// The current foreground window (null while focus is changing).
pub fn foreground() -> HWND {
    // SAFETY: no arguments; returns a handle value that is only compared.
    unsafe { GetForegroundWindow() }
}
