use std::time::{Duration, Instant};

use mm_core::teacher::{confirmation_answer, confirmation_ready_at};

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
