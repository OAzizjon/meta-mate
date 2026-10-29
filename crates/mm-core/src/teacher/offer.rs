use std::time::{Duration, Instant};

/// Queued speech must finish, then leave time for the person to start a new press.
pub fn confirmation_ready_at(now: Instant, queued_until: Instant, prompt: Duration) -> Instant {
    now.max(queued_until) + prompt + Duration::from_millis(300)
}

/// Unknown, interrupted and ambiguous answers keep the offer alive.
pub fn confirmation_answer(text: &str, pressed: Option<Instant>, ready: Instant) -> Option<bool> {
    match crate::api::intent::parse_yes_no(text) {
        Some(false) => Some(false),
        Some(true) if pressed.is_some_and(|at| at > ready) => Some(true),
        _ => None,
    }
}
