//! The user's window a request is about, as opaque ids (no Win32 types).

/// The target window. `handle` is the native window handle as a number (an HWND value on
/// Windows), never dereferenced here; `process_id` guards against a reused handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetWindow {
    pub handle: u64,
    pub process_id: u32,
}
