use std::time::{Duration, Instant};

use mm_core::teacher::{confirmation_answer as checked_answer, confirmation_ready_at};

fn confirmation_answer(text: &str, pressed: Option<Instant>, ready: Instant) -> Option<bool> {
    checked_answer(text, pressed, ready, 0, 1, "en")
}

#[test]
fn intervening_silent_presses_require_a_new_prompt() {
    let ready = Instant::now();
    let press = Some(ready + Duration::from_millis(1));
    for epoch in [0, 2, 3, u64::MAX] {
        // Cancelled, empty and failed recognition presses all increment the same epoch.
        assert_eq!(checked_answer("yes", press, ready, 0, epoch, "en"), None);
        assert_eq!(
            checked_answer("no", press, ready, 0, epoch, "en"),
            Some(false)
        );
    }
    assert_eq!(checked_answer("yes", press, ready, 2, 3, "en"), Some(true));
    assert_eq!(checked_answer("yes", press, ready, u64::MAX, 0, "en"), None);
    assert_eq!(checked_answer("ha", press, ready, 0, 1, "en"), None);
    assert_eq!(checked_answer("ha", press, ready, 0, 1, "uz"), Some(true));
}

#[test]
fn speech_after_a_prompt_closes_the_offer() {
    use mm_core::teacher::{LineKey, ModelChoice, TeacherCommand, closes_confirmation};
    for command in [
        TeacherCommand::Say(LineKey::WrongPlace),
        TeacherCommand::UseModel(ModelChoice::Thorough),
        TeacherCommand::SpeakCommentary("Almost".into()),
    ] {
        assert!(closes_confirmation(&command));
    }
    assert!(!closes_confirmation(&TeacherCommand::UseModel(
        ModelChoice::Quick
    )));
}

#[test]
fn prompt_waits_for_queued_audio_and_a_human_pause() {
    let now = Instant::now();
    let wrong_place = Duration::from_millis(1200);
    let prompt = Duration::from_millis(2400);
    let ready = confirmation_ready_at(now, now + wrong_place, prompt);
    assert_eq!(ready - now, Duration::from_millis(3900));
    assert_eq!(
        confirmation_ready_at(now, now - wrong_place, prompt) - now,
        Duration::from_millis(2700)
    );
    for press in [
        now,
        now + wrong_place,
        ready - Duration::from_millis(1),
        ready,
    ] {
        assert_eq!(confirmation_answer("yes", Some(press), ready), None);
    }
    assert_eq!(
        confirmation_answer("yes", Some(ready + Duration::from_millis(1)), ready),
        Some(true)
    );
    assert_eq!(confirmation_answer("yes", None, ready), None);
}

#[test]
fn live_offer_keeps_unclear_answers_and_refusals_win_even_when_early() {
    let now = Instant::now();
    let ready = confirmation_ready_at(now, now, Duration::from_secs(2));
    let after = Some(ready + Duration::from_millis(1));
    for text in ["ok, what's that", "окей, а какую", "where's it", "maybe"] {
        assert_eq!(confirmation_answer(text, after, ready), None);
    }
    assert_eq!(
        confirmation_answer("нет, не надо, я сам нажму", Some(now), ready),
        Some(false)
    );
    assert_eq!(
        confirmation_answer("okay, do not press it", after, ready),
        Some(false)
    );
    let repeated = confirmation_ready_at(ready, ready, Duration::from_secs(2));
    assert_eq!(confirmation_answer("yes", after, repeated), None);
    assert_eq!(
        confirmation_answer("yes", Some(repeated + Duration::from_millis(1)), repeated),
        Some(true)
    );
}
