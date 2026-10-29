//! Button transitions from a low-level mouse hook on its own message-pump thread.
//! The callback only queues events and always passes input on. Coordinates are
//! physical virtual-screen pixels because the process is Per-Monitor DPI v2 aware.

use std::cell::OnceCell;
use std::fmt;
use std::io;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

use mm_core::geom::PhysPoint;
use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, GetSystemMetrics, HHOOK, LLMHF_INJECTED, LLMHF_LOWER_IL_INJECTED,
    MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SM_CXDOUBLECLK,
    SM_CYDOUBLECLK, SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_USER,
};
use windows::core::PCWSTR;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// Cursor and physical left-button state, independent of WebView pointer capture.
pub fn drag_pointer() -> Option<(PhysPoint, bool)> {
    let mut point = POINT::default();
    // SAFETY: writable stack point and a constant virtual-key code; no input is changed.
    unsafe {
        GetCursorPos(&mut point).ok()?;
        Some((
            PhysPoint {
                x: point.x,
                y: point.y,
            },
            GetAsyncKeyState(i32::from(VK_LBUTTON.0)) < 0,
        ))
    }
}

/// One button transition, including software input (which never confirms a step).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    pub button: Button,
    pub down: bool,
    pub at: PhysPoint,
    pub injected: bool,
    /// When the hook procedure ran.
    pub time: Instant,
}

#[derive(Debug)]
pub enum HookError {
    /// A thread could not be started.
    Thread(io::Error),
    /// `GetModuleHandleW` or `SetWindowsHookExW` failed.
    Install(windows::core::Error),
    /// The hook thread ended before it reported.
    Ended,
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Thread(error) => write!(f, "cannot start a thread: {error}"),
            Self::Install(error) => write!(f, "cannot install the mouse hook: {error}"),
            Self::Ended => f.write_str("the mouse hook thread ended early"),
        }
    }
}

impl std::error::Error for HookError {}

/// Dropping the installed hook ends the pump, which unhooks, and joins the thread.
#[derive(Debug)]
pub struct MouseHook {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl Drop for MouseHook {
    fn drop(&mut self) {
        // SAFETY: a plain thread id and message, no pointers; the pump ends on WM_QUIT.
        let posted = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            // Without the message the pump never ends: never block on it then.
            if posted.is_ok() || thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}

thread_local! {
    /// Set once on the hook thread, read without a lock or a borrow flag.
    static EVENTS: OnceCell<Sender<MouseEvent>> = const { OnceCell::new() };
}

/// Installs the hook on a new thread and returns once it is in place. Injected
/// transitions are sent too, flagged so consumers can exclude them from completion.
pub fn start(events: Sender<MouseEvent>) -> Result<MouseHook, HookError> {
    let (ready, installed) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("mm-mouse-hook".into())
        .spawn(move || pump(events, ready))
        .map_err(HookError::Thread)?;
    match installed.recv() {
        Ok(Ok(thread_id)) => Ok(MouseHook {
            thread_id,
            thread: Some(thread),
        }),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(HookError::Install(error))
        }
        Err(_) => {
            let _ = thread.join();
            Err(HookError::Ended)
        }
    }
}

/// System double-click timeout (milliseconds) and rectangle width/height (physical pixels).
pub fn double_click_rules() -> (u32, i32, i32) {
    // SAFETY: these queries take no pointers, only constant system metric indices.
    unsafe {
        (
            GetDoubleClickTime(),
            GetSystemMetrics(SM_CXDOUBLECLK),
            GetSystemMetrics(SM_CYDOUBLECLK),
        )
    }
}

fn pump(events: Sender<MouseEvent>, ready: Sender<windows::core::Result<u32>>) {
    let _ = EVENTS.with(|cell| cell.set(events));
    let mut msg = MSG::default();
    // SAFETY: `msg` is writable on our stack. Peeking creates the thread's queue
    // before reporting readiness, so Drop's WM_QUIT has a target.
    let _ = unsafe { PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE) };
    let hook = match install() {
        Ok(hook) => hook,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    // SAFETY: no arguments.
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
    // The hook runs inside GetMessageW; thread messages have no window to dispatch to.
    // SAFETY: `msg` is writable on our stack; no window filter. WM_QUIT or error ends it.
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {}
    // SAFETY: `hook` was installed on this thread and is unhooked once.
    let _ = unsafe { UnhookWindowsHookEx(hook) };
}

fn install() -> windows::core::Result<HHOOK> {
    // SAFETY: a null name asks for the executable's module; the handle is not
    // reference-counted and is never freed.
    let module = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
    // SAFETY: `hook_proc` has the HOOKPROC signature and lives as long as the program;
    // thread id 0 makes the hook global, called on this thread's message pump.
    unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), Some(HINSTANCE(module.0)), 0) }
}

fn button_transition(message: u32) -> Option<(Button, bool)> {
    match message {
        WM_LBUTTONDOWN => Some((Button::Left, true)),
        WM_LBUTTONUP => Some((Button::Left, false)),
        WM_RBUTTONDOWN => Some((Button::Right, true)),
        WM_RBUTTONUP => Some((Button::Right, false)),
        WM_MBUTTONDOWN => Some((Button::Middle, true)),
        WM_MBUTTONUP => Some((Button::Middle, false)),
        _ => None,
    }
}

fn injected(flags: u32) -> bool {
    flags & (LLMHF_INJECTED | LLMHF_LOWER_IL_INJECTED) != 0
}

/// Queues button transitions and passes every event on, including moves and wheel events.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Negative codes go straight on; a low-level hook otherwise reports HC_ACTION (0).
    if code >= 0
        && let Some((button, down)) = button_transition(wparam.0 as u32)
    {
        let time = Instant::now();
        // SAFETY: with HC_ACTION, `lparam` points to this event's MSLLHOOKSTRUCT,
        // valid for this call; only plain fields are read.
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let event = MouseEvent {
            button,
            down,
            at: PhysPoint {
                x: info.pt.x,
                y: info.pt.y,
            },
            injected: injected(info.flags),
            time,
        };
        let _ = EVENTS.try_with(|events| {
            if let Some(events) = events.get() {
                let _ = events.send(event);
            }
        });
    }
    // SAFETY: this call's own arguments, unchanged; Windows ignores the hook handle.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_LBUTTONDBLCLK, WM_MBUTTONDBLCLK, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
        WM_RBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP,
    };

    #[test]
    fn maps_all_six_button_transitions() {
        for (message, button, down) in [
            (WM_LBUTTONDOWN, Button::Left, true),
            (WM_LBUTTONUP, Button::Left, false),
            (WM_RBUTTONDOWN, Button::Right, true),
            (WM_RBUTTONUP, Button::Right, false),
            (WM_MBUTTONDOWN, Button::Middle, true),
            (WM_MBUTTONUP, Button::Middle, false),
        ] {
            assert_eq!(button_transition(message), Some((button, down)));
        }
    }

    #[test]
    fn ignores_other_mouse_messages() {
        for message in [
            WM_MOUSEMOVE,
            WM_MOUSEWHEEL,
            WM_MOUSEHWHEEL,
            WM_XBUTTONDOWN,
            WM_XBUTTONUP,
            WM_LBUTTONDBLCLK,
            WM_RBUTTONDBLCLK,
            WM_MBUTTONDBLCLK,
            0,
            u32::MAX,
        ] {
            assert_eq!(button_transition(message), None);
        }
    }

    #[test]
    fn either_injection_flag_marks_software_input() {
        assert!(!injected(0));
        assert!(injected(LLMHF_INJECTED));
        assert!(injected(LLMHF_LOWER_IL_INJECTED));
        assert!(injected(LLMHF_INJECTED | LLMHF_LOWER_IL_INJECTED));
        assert!(!injected(0x8000_0000));
        assert!(injected(0x8000_0000 | LLMHF_LOWER_IL_INJECTED));
    }
}
