//! One MetaMate per Windows session. A named event marks the running app; a second start
//! sets it and leaves, and the first one shows its Home window. Without it, opening the app
//! again from the Start menu (after the installer already started it) would put a second
//! Mate with a second keyboard hook on the screen.

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HANDLE, SetLastError,
    WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForSingleObject};
use windows::core::HSTRING;

/// The running app's wake-up event. Its handle stays open for the life of the process, so
/// the name stays taken while the app runs.
pub struct Instance {
    event: usize,
}

/// How this start relates to a MetaMate already running in the session.
pub enum Start {
    /// The first one, or the check itself failed (then `None`: the app starts anyway).
    First(Option<Instance>),
    /// Another one runs and was asked to show itself; this process should exit.
    Again,
}

/// Claims the session-wide name `Local\<app>.Instance`.
pub fn start(app: &str) -> Start {
    let name = HSTRING::from(format!("Local\\{app}.Instance"));
    // SAFETY: only sets this thread's last error. CreateEventW is documented to set
    // ERROR_ALREADY_EXISTS when it opens an existing event, not to clear it for a new one: a
    // stale value from an earlier call must not turn a first start into `Again`.
    unsafe { SetLastError(ERROR_SUCCESS) };
    // SAFETY: no security attributes; `name` is a NUL-terminated wide string that outlives
    // the call. An auto-reset event, not signalled.
    let event = match unsafe { CreateEventW(None, false, false, &name) } {
        Ok(event) => event,
        Err(error) => {
            eprintln!("instance: {error}");
            return Start::First(None);
        }
    };
    // SAFETY: reads this thread's last error right after CreateEventW succeeded; cleared just
    // before it, it is ERROR_ALREADY_EXISTS exactly when the event existed already.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // SAFETY: `event` is the valid handle CreateEventW just returned; failing to wake the
        // other app only means it does not show Home.
        let _ = unsafe { SetEvent(event) };
        // SAFETY: the handle is ours and closed exactly once.
        let _ = unsafe { CloseHandle(event) };
        return Start::Again;
    }
    Start::First(Some(Instance {
        event: event.0 as usize,
    }))
}

impl Instance {
    /// Calls `show` on a background thread each time another start asks for the app.
    pub fn on_start_again(self, show: impl Fn() + Send + 'static) {
        let raw = self.event;
        let spawned = std::thread::Builder::new()
            .name("instance".into())
            .spawn(move || {
                let event = HANDLE(self.event as *mut core::ffi::c_void);
                loop {
                    // SAFETY: the handle is never closed in this process, so it stays valid
                    // for every wait.
                    if unsafe { WaitForSingleObject(event, INFINITE) } != WAIT_OBJECT_0 {
                        return;
                    }
                    show();
                }
            });
        if let Err(error) = spawned {
            eprintln!("instance: {error}");
            // No waiter: free the name, so a later start runs normally instead of waking
            // nobody and leaving.
            // SAFETY: the thread never started, so nothing else holds or uses this handle.
            let _ = unsafe { CloseHandle(HANDLE(raw as *mut core::ffi::c_void)) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Start, start};
    use std::sync::mpsc;
    use std::time::Duration;
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, SetLastError};

    #[test]
    fn a_stale_last_error_does_not_make_a_first_start_leave() {
        // SAFETY: only sets this thread's last error, as an earlier failed call would.
        unsafe { SetLastError(ERROR_ALREADY_EXISTS) };
        let name = format!("MetaMateStale{}", std::process::id());
        assert!(matches!(start(&name), Start::First(Some(_))));
    }

    #[test]
    fn a_second_start_wakes_the_first_and_is_told_to_leave() {
        let name = format!("MetaMateTest{}", std::process::id());
        let Start::First(Some(first)) = start(&name) else {
            panic!("the first start must own the name");
        };
        let (sender, woken) = mpsc::channel();
        first.on_start_again(move || {
            let _ = sender.send(());
        });
        assert!(matches!(start(&name), Start::Again));
        woken
            .recv_timeout(Duration::from_secs(5))
            .expect("the running instance is asked to show itself");
        assert!(matches!(start(&name), Start::Again));
        woken
            .recv_timeout(Duration::from_secs(5))
            .expect("every later start wakes it again");
    }
}
