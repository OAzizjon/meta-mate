use std::time::{Duration, Instant};

/// Queued speech must finish, then leave time for the person to start a new press.
pub fn confirmation_ready_at(now: Instant, queued_until: Instant, prompt: Duration) -> Instant {
    now.max(queued_until) + prompt + Duration::from_millis(300)
}

/// Unknown, interrupted and ambiguous answers keep the offer alive.
pub fn confirmation_answer(
    text: &str,
    pressed: Option<Instant>,
    ready: Instant,
    prompt_epoch: u64,
    speech_epoch: u64,
    locale: &str,
) -> Option<bool> {
    match crate::api::intent::parse_yes_no_for_locale(text, locale) {
        Some(false) => Some(false),
        Some(true)
            if pressed.is_some_and(|at| at > ready)
                && prompt_epoch.checked_add(1) == Some(speech_epoch) =>
        {
            Some(true)
        }
        _ => None,
    }
}

/// Any later speech closes an offer: a yes must answer the confirmation prompt.
pub fn closes_confirmation(command: &super::TeacherCommand) -> bool {
    matches!(
        command,
        super::TeacherCommand::Say(_)
            | super::TeacherCommand::SpeakCommentary(_)
            | super::TeacherCommand::UseModel(super::ModelChoice::Thorough)
    )
}
