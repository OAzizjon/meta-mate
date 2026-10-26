//! Push-to-talk key and mouse click events from the global hooks.

use mm_core::geom::PhysPoint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// One global input event. `time_ms` is the event time on the system's monotonic clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    /// The push-to-talk key went down (`pressed`) or up; auto-repeat is never reported.
    PushToTalk { pressed: bool, time_ms: u64 },
    /// A mouse button went down at a physical virtual-screen point (step checks).
    MouseClick {
        button: MouseButton,
        at: PhysPoint,
        time_ms: u64,
    },
}

/// Global key and mouse events. Hook callbacks only enqueue (Windows silently removes slow
/// hooks); the caller drains the queue.
pub trait InputHooks {
    /// The next queued event, or `None` when the queue is empty. Never blocks.
    fn poll_event(&mut self) -> Option<InputEvent>;
}
