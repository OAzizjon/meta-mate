//! Window changes on an out-of-context hook thread. The callback only queues ids.

use std::cell::OnceCell;
use std::io;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use mm_core::geom::PhysRect;
use mm_platform::TargetWindow;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_QUIT, WM_USER,
};

pub const LOCATION: u32 = 0x800b;
// The hooked range: EVENT_SYSTEM_SCROLLINGSTART (0x0012) up to EVENT_OBJECT_CONTENTSCROLLED
// (0x8015); the follower (src-tauri/src/pointing.rs) acts on a few of them only.
const FIRST_EVENT: u32 = 0x0012;
const LAST_EVENT: u32 = 0x8015;

// These two user32 APIs otherwise require the Accessibility Cargo feature. Keep the
// small ABI here so this package needs no changes outside its three assigned files.
type Hook = *mut std::ffi::c_void;
type Callback = unsafe extern "system" fn(Hook, u32, HWND, i32, i32, u32, u32);
// SAFETY: user32's WinEvent declarations use opaque handles, fixed-width integers
// and a system-ABI callback. No pointers to Rust-owned data cross this boundary.
#[link(name = "user32")]
unsafe extern "system" {
    fn SetWinEventHook(
        first: u32,
        last: u32,
        module: Hook,
        callback: Option<Callback>,
        process: u32,
        thread: u32,
        flags: u32,
    ) -> Hook;
    fn UnhookWinEvent(hook: Hook) -> i32;
}

#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub event: u32,
    pub hwnd: u64,
    pub object: i32,
    pub child: i32,
}

thread_local! {
    static EVENTS: OnceCell<Sender<Event>> = const { OnceCell::new() };
}

/// Dropping the hook stops its message loop and unhooks on the installing thread.
pub struct WindowHook {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl Drop for WindowHook {
    fn drop(&mut self) {
        // SAFETY: only integer message arguments; the queue exists before start returns.
        let posted = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take()
            && (posted.is_ok() || thread.is_finished())
        {
            let _ = thread.join();
        }
    }
}

pub fn start(target: TargetWindow, events: Sender<Event>) -> io::Result<WindowHook> {
    let (ready, installed) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("mm-window-hook".into())
        .spawn(move || {
            let _ = EVENTS.with(|cell| cell.set(events));
            let mut msg = MSG::default();
            // SAFETY: writable local MSG; creates the queue before reporting readiness.
            let _ = unsafe { PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE) };
            // SAFETY: callback has the WinEvent ABI and static lifetime. Null module,
            // thread 0 and flags 2 mean OUTOFCONTEXT (0) | SKIPOWNPROCESS (2).
            let hook = unsafe {
                SetWinEventHook(
                    FIRST_EVENT,
                    LAST_EVENT,
                    std::ptr::null_mut(),
                    Some(callback),
                    target.process_id,
                    0,
                    2,
                )
            };
            if hook.is_null() {
                let _ = ready.send(Err(io::Error::last_os_error()));
                return;
            }
            // SAFETY: no arguments.
            let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
            // SAFETY: writable local MSG; out-of-context callbacks run in this pump.
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {}
            // SAFETY: installed on this thread, unhooked exactly once here.
            let _ = unsafe { UnhookWinEvent(hook) };
        })?;
    match installed.recv() {
        Ok(Ok(thread_id)) => Ok(WindowHook {
            thread_id,
            thread: Some(thread),
        }),
        result => {
            let _ = thread.join();
            Err(match result {
                Ok(Err(error)) => error,
                _ => io::Error::other("window hook thread ended early"),
            })
        }
    }
}

unsafe extern "system" fn callback(
    _: Hook,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    let _ = EVENTS.try_with(|cell| {
        if let Some(events) = cell.get() {
            let _ = events.send(Event {
                event,
                hwnd: hwnd.0 as usize as u64,
                object,
                child,
            });
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow {
    Same,
    Shift(i32, i32),
    Clear,
}

/// Only equal-sized valid frames with a representable translation can be followed.
pub fn follow(old: PhysRect, new: PhysRect) -> Follow {
    let size = |r: PhysRect| {
        (
            i64::from(r.right) - i64::from(r.left),
            i64::from(r.bottom) - i64::from(r.top),
        )
    };
    let (width, height) = size(old);
    if width <= 0 || height <= 0 || size(new) != (width, height) {
        return Follow::Clear;
    }
    let (Some(dx), Some(dy)) = (new.left.checked_sub(old.left), new.top.checked_sub(old.top))
    else {
        return Follow::Clear;
    };
    if dx == 0 && dy == 0 {
        Follow::Same
    } else {
        Follow::Shift(dx, dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_and_translated_frames() {
        let old = PhysRect::new(10, 20, 110, 220).unwrap();
        assert_eq!(follow(old, old), Follow::Same);
        assert_eq!(
            follow(old, PhysRect::new(-40, -10, 60, 190).unwrap()),
            Follow::Shift(-50, -30)
        );
        assert_eq!(
            follow(old, PhysRect::new(10, 25, 110, 225).unwrap()),
            Follow::Shift(0, 5)
        );
    }

    #[test]
    fn resize_invalid_and_overflow_clear() {
        let old = PhysRect::new(0, 0, 100, 200).unwrap();
        assert_eq!(
            follow(old, PhysRect::new(0, 0, 101, 200).unwrap()),
            Follow::Clear
        );
        assert_eq!(
            follow(old, PhysRect::new(0, 0, 100, 201).unwrap()),
            Follow::Clear
        );
        let invalid = PhysRect { right: 0, ..old };
        assert_eq!(follow(invalid, invalid), Follow::Clear);
        let left = PhysRect::new(i32::MIN, 0, i32::MIN + 10, 10).unwrap();
        let right = PhysRect::new(i32::MAX - 10, 0, i32::MAX, 10).unwrap();
        assert_eq!(follow(left, right), Follow::Clear);
    }
}
