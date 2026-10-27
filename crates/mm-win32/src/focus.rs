//! The user's foreground window: the one our windows must never take over (invariant 1).

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// The current foreground window (null while focus is changing).
pub fn foreground() -> HWND {
    // SAFETY: no arguments; returns a handle value that is only compared.
    unsafe { GetForegroundWindow() }
}
