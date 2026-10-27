//! Our windows on top of the user's app without ever taking focus (invariant 1):
//! `WS_EX_NOACTIVATE` and `SWP_NOACTIVATE` everywhere, `SW_SHOWNOACTIVATE` to show.
//! Never `WDA_EXCLUDEFROMCAPTURE` (Zoom and OBS must still show Mate): hiding for our own
//! capture is a short `hide`.

use mm_core::geom::PhysRect;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, IsWindowVisible, SET_WINDOW_POS_FLAGS, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, WINDOW_EX_STYLE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT,
};

/// Which of our windows: Mate himself takes the mouse (drag, right click) but never
/// focus; the highlight lets every click through to the app underneath.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    Robot,
    Highlight,
}

impl OverlayKind {
    /// The extended styles this kind adds to whatever the window already has.
    pub fn ex_style(self) -> WINDOW_EX_STYLE {
        let base = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
        match self {
            Self::Robot => base,
            Self::Highlight => base | WS_EX_LAYERED | WS_EX_TRANSPARENT,
        }
    }
}

/// The window's current extended style bits.
pub fn ex_style(hwnd: HWND) -> WINDOW_EX_STYLE {
    // SAFETY: reads a window long; an invalid handle returns 0.
    WINDOW_EX_STYLE(unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32)
}

/// Adds the no-activate styles (keeping every bit already set: WebView2 and tao set
/// their own) and makes the window topmost without activating it. Call it while the
/// window is still hidden, before the first show.
pub fn prepare(hwnd: HWND, kind: OverlayKind) -> windows::core::Result<()> {
    let style = ex_style(hwnd) | kind.ex_style();
    // SAFETY: writes the extended style of a window this process owns; the value only
    // adds documented WS_EX bits to the existing ones.
    unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style.0 as isize) };
    // SAFETY: plain handle and flags; SWP_FRAMECHANGED applies the new style and
    // SWP_NOACTIVATE keeps the user's window in front of the keyboard.
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
    }?;
    // Never show a window whose no-activate bits did not stick (invariant 1).
    if ex_style(hwnd).contains(kind.ex_style()) {
        Ok(())
    } else {
        Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_FAIL,
        ))
    }
}

/// Shows the window without activating it (never Tauri's `show`, which activates).
pub fn show(hwnd: HWND) {
    // SAFETY: plain handle and command; the return value is the previous visibility.
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
}

/// Hides the window (for one capture frame, or to sleep in the tray).
pub fn hide(hwnd: HWND) {
    // SAFETY: plain handle and command.
    let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
}

/// Moves and resizes the window to `rect` (physical pixels), topmost, without
/// activating it: an open dropdown in the user's app stays open.
pub fn place(hwnd: HWND, rect: PhysRect) -> windows::core::Result<()> {
    // SAFETY: plain handle, coordinates and flags.
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOACTIVATE,
        )
    }
}

/// Hides the window for one capture frame (never `WDA_EXCLUDEFROMCAPTURE`, see above):
/// no activation, no move, no z-order change.
pub fn hide_for_capture(hwnd: HWND) {
    set_shown_quietly(hwnd, SWP_HIDEWINDOW);
}

/// True while the window is hidden: a hide that did not take (a handle from another
/// thread's window being destroyed, a shell refusing) would put Mate in the picture.
pub fn is_hidden(hwnd: HWND) -> bool {
    // SAFETY: a plain handle query; a stale handle reads as not visible.
    !unsafe { IsWindowVisible(hwnd).as_bool() }
}

/// Waits until the desktop compositor has presented a frame: a window hidden before the
/// call is gone from the screen a copy reads after it. False when composition is off or
/// the call failed (the caller then waits a fixed frame instead).
pub fn wait_for_composition() -> bool {
    // SAFETY: no arguments; blocks until the next DWM present.
    unsafe { windows::Win32::Graphics::Dwm::DwmFlush() }.is_ok()
}

/// Shows the window again after the capture, still without activating it (invariant 1).
pub fn show_after_capture(hwnd: HWND) {
    set_shown_quietly(hwnd, SWP_SHOWWINDOW);
}

/// Hides or shows the window and changes nothing else.
fn set_shown_quietly(hwnd: HWND, visibility: SET_WINDOW_POS_FLAGS) {
    // SAFETY: plain handle and flags; SWP_NOZORDER ignores the missing insert-after
    // handle and SWP_NOACTIVATE keeps the user's window in front of the keyboard.
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            visibility
                | SWP_NOACTIVATE
                | SWP_NOMOVE
                | SWP_NOSIZE
                | SWP_NOZORDER
                | SWP_NOOWNERZORDER,
        )
    };
}
