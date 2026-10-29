//! The push-to-talk key from a low-level keyboard hook (spike S2). `RegisterHotKey` has
//! no key-up, so `WH_KEYBOARD_LL` runs on a thread of its own with a message pump.
//! Windows silently removes a slow hook, so the hook procedure only queues a `KeyEvent`
//! and passes keys on, except Caps Lock when selected as the talk key.
//! `key_down` is the release watchdog for a key-up that never arrived
//! (docs/edge.md section 5).

use std::cell::OnceCell;
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_RCONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, HHOOK,
    KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLKHF_LOWER_IL_INJECTED, LLKHF_UP, MSG, PM_NOREMOVE,
    PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYUP, WM_QUIT, WM_SYSKEYUP, WM_USER,
};
use windows::core::PCWSTR;

/// Right Ctrl's virtual-key code as a low-level hook reports it (`VK_RCONTROL`, 0xA3).
pub const RIGHT_CTRL: u32 = VK_RCONTROL.0 as u32;
static PTT_KEY: AtomicU32 = AtomicU32::new(RIGHT_CTRL);

pub fn set_ptt_key(vk: u32) {
    PTT_KEY.store(vk, Ordering::Relaxed);
}

/// One key transition as the hook saw it: the virtual-key code only, never a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub foreground: Option<mm_core::teacher::KeyWindow>,
    pub vk: u32,
    pub down: bool,
    /// Either injected flag: software (`SendInput`, ours included) made it, not a person.
    pub injected: bool,
    /// When the hook procedure ran.
    pub at: Instant,
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
            Self::Install(error) => write!(f, "cannot install the keyboard hook: {error}"),
            Self::Ended => f.write_str("the keyboard hook thread ended early"),
        }
    }
}

impl std::error::Error for HookError {}

/// The installed hook. Dropping it ends the pump, which unhooks, and joins the thread.
#[derive(Debug)]
pub struct KeyboardHook {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl Drop for KeyboardHook {
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
    /// Where the hook procedure queues events: set once on the hook thread, read without
    /// a lock or a borrow flag.
    static EVENTS: OnceCell<Box<dyn Fn(KeyEvent)>> = const { OnceCell::new() };
}

/// Installs the hook on a new thread and returns once it is in place. Every key
/// transition anywhere is sent to `events`, injected ones included (flagged).
pub fn start(events: Sender<KeyEvent>) -> Result<KeyboardHook, HookError> {
    start_mapped(events, |event| event)
}

/// Queues a mapped event directly, without a forwarding thread. The mapper must stay
/// trivial: it runs inside the low-level hook, before Windows updates async key state.
pub fn start_mapped<T: Send + 'static>(
    events: Sender<T>,
    map: fn(KeyEvent) -> T,
) -> Result<KeyboardHook, HookError> {
    let (ready, installed) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("mm-keyboard-hook".into())
        .spawn(move || {
            pump(
                Box::new(move |event| {
                    let _ = events.send(map(event));
                }),
                ready,
            )
        })
        .map_err(HookError::Thread)?;
    match installed.recv() {
        Ok(Ok(thread_id)) => Ok(KeyboardHook {
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

/// Whether the chosen virtual key is down right now (the high bit of `GetAsyncKeyState`). Reads up
/// while another desktop (UAC, the lock screen) is active.
pub fn key_down(vk: u32) -> bool {
    // SAFETY: a plain virtual-key code, no pointers.
    let state = unsafe { GetAsyncKeyState(vk as i32) };
    state < 0
}

/// The hook thread: installs the hook, reports, pumps messages until WM_QUIT, unhooks.
fn pump(events: Box<dyn Fn(KeyEvent)>, ready: Sender<windows::core::Result<u32>>) {
    let _ = EVENTS.with(|cell| cell.set(events));
    let mut msg = MSG::default();
    // SAFETY: `msg` is a writable MSG on our stack. Peeking without removing anything
    // makes Windows create this thread's queue, so `Drop`'s WM_QUIT always has a target.
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
    // The hook procedure is called inside GetMessageW. Thread messages have no window to
    // dispatch to; WM_QUIT (0) or an error (-1) ends the pump.
    // SAFETY: `msg` is a writable MSG on our stack; no window filter.
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {}
    // SAFETY: `hook` came from SetWindowsHookExW on this thread and is unhooked once.
    let _ = unsafe { UnhookWindowsHookEx(hook) };
}

fn install() -> windows::core::Result<HHOOK> {
    // SAFETY: a null name asks for the running executable's module; the handle is not
    // reference-counted and is never freed.
    let module = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
    // SAFETY: `hook_proc` has the HOOKPROC signature and lives as long as the program;
    // thread id 0 makes the hook global, called on this thread's message pump.
    unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(hook_proc),
            Some(HINSTANCE(module.0)),
            0,
        )
    }
}

/// Queues the transition and passes the key on, always.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // A negative code goes straight on; a low-level hook gets only HC_ACTION (0) otherwise.
    if code >= 0 {
        let at = Instant::now();
        // SAFETY: with HC_ACTION, `lparam` points to this event's KBDLLHOOKSTRUCT, valid
        // for the duration of the call; only plain fields are read from it.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let up =
            (info.flags.0 & LLKHF_UP.0) != 0 || matches!(wparam.0 as u32, WM_KEYUP | WM_SYSKEYUP);
        // SAFETY: plain window queries; the PID out pointer is local to this callback.
        let foreground = unsafe {
            let hwnd = GetForegroundWindow();
            let mut process_id = 0;
            let thread = GetWindowThreadProcessId(hwnd, Some(&mut process_id));
            (thread != 0 && process_id != 0).then_some(mm_core::teacher::KeyWindow {
                handle: hwnd.0 as u64,
                process_id,
            })
        };
        let event = KeyEvent {
            foreground,
            vk: info.vkCode,
            down: !up,
            injected: (info.flags.0 & (LLKHF_INJECTED.0 | LLKHF_LOWER_IL_INJECTED.0)) != 0,
            at,
        };
        let _ = EVENTS.try_with(|events| {
            if let Some(events) = events.get() {
                events(event);
            }
        });
        // Keep Caps Lock's state and light unchanged while it is the talk key.
        // Still queue both transitions above for PTT and its release watchdog.
        if mm_core::ptt::suppress_lock_key(
            event.vk,
            PTT_KEY.load(Ordering::Relaxed),
            event.injected,
        ) {
            return LRESULT(1);
        }
    }
    // SAFETY: this call's own arguments, unchanged; the hook handle is ignored by Windows.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
