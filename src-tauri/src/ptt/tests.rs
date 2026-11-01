use super::*;

#[test]
fn clicks_toggle_with_unique_generations_and_ignore_the_key_watchdog() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    assert_eq!(feed.click(t0), start(1));
    assert_eq!(feed.pressed_at, None);
    assert_eq!(feed.poll(false, at(t0, 100)), nothing());
    assert_eq!(feed.click(at(t0, 200)), stop(1, StopReason::Toggled));
    assert_eq!(feed.click(at(t0, 300)), start(2));
    assert_eq!(feed.silence(1), nothing());
    assert_eq!(feed.silence(2), stop(2, StopReason::Toggled));
    assert_eq!(feed.click(at(t0, 400)), start(3));
    assert_eq!(feed.tick(at(t0, 30_399)), nothing());
    assert_eq!(feed.tick(at(t0, 30_400)), stop(3, StopReason::TooLong));
}

#[test]
fn key_press_stops_click_without_reopening_on_repeat_or_release() {
    for mode in [Mode::Hold, Mode::Toggle] {
        let t0 = Instant::now();
        let mut feed = Feed::new(PttSettings::default(), t0);
        assert_eq!(feed.click(t0), start(1));
        assert_eq!(
            feed.key_in_mode(injected(RIGHT_CTRL, true, at(t0, 50)), mode),
            nothing()
        );
        assert_eq!(
            feed.key_in_mode(key(RIGHT_CTRL, true, at(t0, 100)), mode),
            stop(1, StopReason::Toggled)
        );
        assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 200))), nothing());
        assert_eq!(feed.tick(at(t0, 600)), nothing());
        assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 700))), nothing());
        assert_eq!(feed.click(at(t0, 800)), start(2));
    }
}

#[test]
fn click_stops_a_key_recording_and_suppresses_a_pending_hold() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    feed.key(key(RIGHT_CTRL, true, t0));
    assert_eq!(feed.tick(at(t0, 250)), start(1));
    assert_eq!(feed.click(at(t0, 300)), stop(1, StopReason::Toggled));
    assert_eq!(feed.tick(at(t0, 600)), nothing());
    assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 700))), nothing());
    feed.key(key(RIGHT_CTRL, true, at(t0, 800)));
    assert_eq!(feed.click(at(t0, 850)), nothing());
    assert_eq!(feed.tick(at(t0, 1100)), nothing());
    assert_eq!(feed.poll(false, at(t0, 1200)), nothing());
    assert_eq!(feed.click(at(t0, 1300)), start(2));

    let mut feed = toggle(t0);
    assert_eq!(tap(&mut feed, t0, 0), start(1));
    assert_eq!(feed.click(at(t0, 200)), stop(1, StopReason::Toggled));
    assert_eq!(feed.click(at(t0, 300)), start(2));
}

#[test]
fn chosen_keys_drive_holds_chords_and_release_watchdog() {
    for vk in [RIGHT_CTRL, 0xA5, 0x14, 0x2D, 0x78, 0x79] {
        let t0 = Instant::now();
        let mut feed = Feed::new(PttSettings::default(), t0);
        feed.set_key(vk);
        assert_eq!(feed.key(key(vk, true, t0)), nothing());
        assert_eq!(feed.tick(at(t0, 250)), start(1));
        assert_eq!(feed.poll(false, at(t0, 300)), stop(1, StopReason::Watchdog));
        feed.key(key(vk, true, at(t0, 400)));
        assert_eq!(feed.tick(at(t0, 650)), start(2));
        assert_eq!(
            feed.key(key(A, true, at(t0, 700))),
            vec![PttEvent::Cancel { generation: 2 }]
        );
        assert_eq!(feed.key(key(vk, false, at(t0, 750))), nothing());
    }
}

#[test]
fn changing_key_cancels_and_old_key_becomes_an_other_key() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    feed.key(key(RIGHT_CTRL, true, t0));
    assert_eq!(feed.tick(at(t0, 250)), start(1));
    assert_eq!(feed.set_key(0x78), stop(1, StopReason::Cancelled));
    assert!(!feed.needs_poll());
    feed.key(key(RIGHT_CTRL, false, at(t0, 300)));
    feed.key(key(0x78, true, at(t0, 400)));
    assert_eq!(feed.tick(at(t0, 650)), start(2));
    assert_eq!(
        feed.key(key(RIGHT_CTRL, true, at(t0, 700))),
        vec![PttEvent::Cancel { generation: 2 }]
    );
}

/// Any other key; only its code matters.
const A: u32 = 0x41;

fn at(t0: Instant, ms: u64) -> Instant {
    t0 + Duration::from_millis(ms)
}

fn key(vk: u32, down: bool, at: Instant) -> KeyEvent {
    KeyEvent {
        foreground: None,
        vk,
        down,
        injected: false,
        at,
    }
}

fn injected(vk: u32, down: bool, at: Instant) -> KeyEvent {
    KeyEvent {
        injected: true,
        ..key(vk, down, at)
    }
}

fn nothing() -> Vec<PttEvent> {
    Vec::new()
}

#[test]
fn tap_preserves_injected_flags_and_drop_disconnects() {
    let tap = Arc::new(KeyTap::default());
    let (guard, received) = tap.subscribe().unwrap();
    let event = injected(A, true, Instant::now());
    tap.send(event);
    assert_eq!(received.try_recv(), Ok(event));
    drop(guard);
    assert_eq!(received.try_recv(), Err(mpsc::TryRecvError::Disconnected));
}

#[test]
fn press_time_precedes_hold_start_and_survives_repeats_and_release() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    assert_eq!(feed.key(key(RIGHT_CTRL, true, t0)), nothing());
    assert_eq!(feed.tick(at(t0, 250)), start(1));
    feed.key(key(RIGHT_CTRL, true, at(t0, 300)));
    feed.key(injected(RIGHT_CTRL, true, at(t0, 350)));
    assert_eq!(feed.pressed_at, Some(t0));
    feed.key(key(RIGHT_CTRL, false, at(t0, 400)));
    assert_eq!(feed.pressed_at, Some(t0));
    feed.key(key(RIGHT_CTRL, true, at(t0, 500)));
    assert_eq!(feed.pressed_at, Some(at(t0, 500)));
}

#[test]
fn old_guard_does_not_cancel_replacement_subscription() {
    let tap = Arc::new(KeyTap::default());
    let (old, old_received) = tap.subscribe().unwrap();
    let (_new, received) = tap.subscribe().unwrap();
    assert_eq!(
        old_received.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    );
    drop(old);
    let event = key(A, true, Instant::now());
    tap.send(event);
    assert_eq!(received.try_recv(), Ok(event));
}

#[test]
fn subscription_does_not_keep_driver_alive() {
    let tap = Arc::new(KeyTap::default());
    let published = Arc::downgrade(&tap);
    let (_guard, received) = tap.subscribe().unwrap();
    drop(tap);
    assert!(published.upgrade().is_none());
    assert_eq!(received.try_recv(), Err(mpsc::TryRecvError::Disconnected));
}

#[test]
fn closed_receiver_is_discarded_without_affecting_ptt() {
    let tap = Arc::new(KeyTap::default());
    let (_guard, received) = tap.subscribe().unwrap();
    drop(received);
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    let event = key(RIGHT_CTRL, true, t0);
    tap.send(event);
    assert!(tap.subscriber.lock().unwrap().is_none());
    assert_eq!(feed.key(event), nothing());
    assert_eq!(feed.tick(at(t0, 250)), start(1));
}

fn start(generation: u64) -> Vec<PttEvent> {
    vec![PttEvent::Start { generation }]
}

fn stop(generation: u64, reason: StopReason) -> Vec<PttEvent> {
    vec![PttEvent::Stop { generation, reason }]
}

#[test]
fn hold_then_release_starts_and_stops() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 0))), nothing());
    assert_eq!(feed.tick(at(t0, 249)), nothing());
    assert!(!feed.active());
    assert_eq!(feed.tick(at(t0, 250)), start(1));
    assert!(feed.active());
    // Auto-repeat and a poll that sees the key held change nothing.
    assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 280))), nothing());
    assert_eq!(feed.poll(true, at(t0, 313)), nothing());
    assert_eq!(
        feed.key(key(RIGHT_CTRL, false, at(t0, 400))),
        stop(1, StopReason::Released)
    );
    assert!(!feed.active());
}

#[test]
fn a_pending_press_keeps_polling_after_a_chord_until_the_release_is_seen() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    assert!(!feed.needs_poll());
    assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 0))), nothing());
    assert!(feed.needs_poll());
    assert_eq!(feed.tick(at(t0, 250)), start(1));
    assert_eq!(
        feed.key(key(A, true, at(t0, 300))),
        vec![PttEvent::Cancel { generation: 1 }]
    );
    // The session is over, but the talk key's key-up was lost: polling goes on.
    assert!(!feed.active());
    assert!(feed.needs_poll());
    assert_eq!(feed.key(key(A, false, at(t0, 350))), nothing());
    assert_eq!(feed.poll(false, at(t0, 400)), nothing());
    assert!(!feed.needs_poll());
    // The next press works.
    assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 500))), nothing());
    assert_eq!(feed.tick(at(t0, 750)), start(2));
}

#[test]
fn another_key_during_the_hold_starts_nothing() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 0))), nothing());
    assert_eq!(feed.key(key(A, true, at(t0, 100))), nothing());
    assert_eq!(feed.tick(at(t0, 300)), nothing());
    assert_eq!(feed.key(key(A, false, at(t0, 350))), nothing());
    assert_eq!(feed.tick(at(t0, 400)), nothing());
    assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 500))), nothing());
    assert!(!feed.active());
}

#[test]
fn another_key_after_the_start_cancels() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    feed.key(key(RIGHT_CTRL, true, at(t0, 0)));
    assert_eq!(feed.tick(at(t0, 250)), start(1));
    assert_eq!(
        feed.key(key(A, true, at(t0, 300))),
        vec![PttEvent::Cancel { generation: 1 }]
    );
    assert!(!feed.active());
}

#[test]
fn lost_key_up_is_caught_by_the_poll() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    feed.key(key(RIGHT_CTRL, true, at(t0, 0)));
    assert_eq!(feed.tick(at(t0, 250)), start(1));
    assert_eq!(feed.poll(true, at(t0, 283)), nothing());
    // The key-up never reaches the hook; only the async key state says "up".
    assert_eq!(feed.tick(at(t0, 316)), nothing());
    assert_eq!(feed.poll(false, at(t0, 316)), stop(1, StopReason::Watchdog));
    assert!(!feed.active());
    // The next press works again.
    feed.key(key(RIGHT_CTRL, true, at(t0, 1_000)));
    assert_eq!(feed.tick(at(t0, 1_250)), start(2));
}

#[test]
fn injected_events_are_ignored() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    assert_eq!(feed.key(injected(RIGHT_CTRL, true, at(t0, 0))), nothing());
    assert_eq!(feed.tick(at(t0, 300)), nothing());
    assert!(!feed.active());
    feed.key(key(RIGHT_CTRL, true, at(t0, 400)));
    assert_eq!(feed.tick(at(t0, 650)), start(1));
    assert_eq!(
        feed.key(injected(RIGHT_CTRL, false, at(t0, 700))),
        nothing()
    );
    assert_eq!(feed.key(injected(A, true, at(t0, 710))), nothing());
    assert!(feed.active());
    assert_eq!(
        feed.key(key(RIGHT_CTRL, false, at(t0, 800))),
        stop(1, StopReason::Released)
    );
}

#[test]
fn one_clock_from_the_driver_start() {
    let t0 = Instant::now();
    let feed = Feed::new(PttSettings::default(), at(t0, 100));
    assert_eq!(feed.ms(t0), 0);
    assert_eq!(feed.ms(at(t0, 350)), 250);
}

fn toggle(t0: Instant) -> Feed {
    Feed::new(
        PttSettings {
            mode: Mode::Toggle,
            ..PttSettings::default()
        },
        t0,
    )
}

fn tap(feed: &mut Feed, t0: Instant, ms: u64) -> Vec<PttEvent> {
    assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, ms))), nothing());
    feed.key(key(RIGHT_CTRL, false, at(t0, ms + 100)))
}

#[test]
fn toggle_taps_start_and_stop_without_watchdog_stopping_the_pause() {
    let t0 = Instant::now();
    let mut feed = toggle(t0);
    assert_eq!(tap(&mut feed, t0, 0), start(1));
    assert_eq!(feed.poll(false, at(t0, 500)), nothing());
    assert_eq!(feed.tick(at(t0, 1_000)), nothing());
    assert_eq!(tap(&mut feed, t0, 1_100), stop(1, StopReason::Toggled));
    assert_eq!(tap(&mut feed, t0, 1_500), start(2));
}

#[test]
fn lost_short_release_does_not_toggle_a_latched_recording() {
    let t0 = Instant::now();
    let mut feed = toggle(t0);
    assert_eq!(tap(&mut feed, t0, 0), start(1));
    feed.key(key(RIGHT_CTRL, true, at(t0, 200)));
    assert_eq!(feed.poll(false, at(t0, 300)), nothing());
    assert!(feed.active());
    assert_eq!(tap(&mut feed, t0, 400), stop(1, StopReason::Toggled));
    // An old silence message must not discard the next pending gesture.
    feed.key(key(RIGHT_CTRL, true, at(t0, 600)));
    assert_eq!(feed.silence(1), nothing());
    assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 700))), start(2));
}

#[test]
fn toggle_long_hold_and_watchdog_release() {
    for watchdog in [false, true] {
        let t0 = Instant::now();
        let mut feed = toggle(t0);
        assert_eq!(feed.key(key(RIGHT_CTRL, true, t0)), nothing());
        assert_eq!(feed.tick(at(t0, 399)), nothing());
        assert_eq!(feed.tick(at(t0, 400)), start(1));
        assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 500))), nothing());
        let out = if watchdog {
            feed.poll(false, at(t0, 600))
        } else {
            feed.key(key(RIGHT_CTRL, false, at(t0, 600)))
        };
        assert_eq!(
            out,
            stop(
                1,
                if watchdog {
                    StopReason::Watchdog
                } else {
                    StopReason::Released
                }
            )
        );
        assert!(!feed.active());
        assert_eq!(feed.tick(at(t0, 1_000)), nothing());
    }
}

#[test]
fn toggle_tap_boundary_is_strict() {
    let t0 = Instant::now();
    let mut feed = toggle(t0);
    feed.key(key(RIGHT_CTRL, true, t0));
    assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 399))), start(1));
    feed.silence(1);
    feed.key(key(RIGHT_CTRL, true, at(t0, 500)));
    // No tick before release: a 400 ms press still cannot latch recording.
    assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 900))), nothing());
    assert!(!feed.active());
}

#[test]
fn toggle_silence_is_generation_checked_and_does_not_reopen_held_key() {
    let t0 = Instant::now();
    let mut feed = toggle(t0);
    assert_eq!(tap(&mut feed, t0, 0), start(1));
    assert_eq!(feed.silence(1), stop(1, StopReason::Toggled));
    assert_eq!(feed.silence(1), nothing());
    assert_eq!(tap(&mut feed, t0, 200), start(2));
    assert_eq!(feed.silence(1), nothing());
    assert!(feed.active());
    assert_eq!(feed.silence(2), stop(2, StopReason::Toggled));
    feed.key(key(RIGHT_CTRL, true, at(t0, 400)));
    assert_eq!(feed.tick(at(t0, 800)), start(3));
    assert_eq!(feed.silence(3), stop(3, StopReason::Toggled));
    assert_eq!(feed.tick(at(t0, 1_000)), nothing());
    assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 1_100))), nothing());
}

#[test]
fn toggle_chords_injection_and_lost_taps_do_not_start() {
    let t0 = Instant::now();
    let mut feed = toggle(t0);
    feed.key(injected(RIGHT_CTRL, true, t0));
    assert_eq!(feed.tick(at(t0, 500)), nothing());
    feed.key(key(RIGHT_CTRL, true, at(t0, 600)));
    assert_eq!(feed.poll(false, at(t0, 700)), nothing());
    assert_eq!(feed.tick(at(t0, 1_000)), nothing());
    feed.key(key(A, true, at(t0, 1_100)));
    assert_eq!(tap(&mut feed, t0, 1_200), nothing());
    feed.key(key(A, false, at(t0, 1_400)));
    assert_eq!(tap(&mut feed, t0, 1_500), start(1));
    feed.key(key(RIGHT_CTRL, true, at(t0, 1_700)));
    assert_eq!(
        feed.key(key(A, true, at(t0, 1_750))),
        vec![PttEvent::Cancel { generation: 1 }]
    );
    assert_eq!(feed.poll(false, at(t0, 1_800)), nothing());
}

#[test]
fn toggle_deadline_is_thirty_seconds_even_without_speech() {
    let t0 = Instant::now();
    let mut feed = toggle(t0);
    assert_eq!(tap(&mut feed, t0, 0), start(1));
    assert_eq!(feed.tick(at(t0, 30_099)), nothing());
    assert_eq!(feed.tick(at(t0, 30_100)), stop(1, StopReason::TooLong));
    assert_eq!(feed.tick(at(t0, 31_000)), nothing());
    assert_eq!(tap(&mut feed, t0, 31_100), start(2));
}

#[test]
fn mode_is_sampled_on_a_fresh_press_and_generations_remain_unique() {
    let t0 = Instant::now();
    let mut feed = toggle(t0);
    assert_eq!(tap(&mut feed, t0, 0), start(1));
    assert_eq!(
        feed.key_in_mode(key(RIGHT_CTRL, true, at(t0, 200)), Mode::Hold),
        stop(1, StopReason::Cancelled)
    );
    assert_eq!(feed.tick(at(t0, 450)), start(2));
    assert_eq!(
        feed.key_in_mode(key(RIGHT_CTRL, true, at(t0, 500)), Mode::Toggle),
        nothing()
    );
    assert_eq!(feed.silence(2), nothing());
    assert_eq!(
        feed.key(key(RIGHT_CTRL, false, at(t0, 600))),
        stop(2, StopReason::Released)
    );
    assert_eq!(
        feed.key_in_mode(key(RIGHT_CTRL, true, at(t0, 700)), Mode::Toggle),
        nothing()
    );
    assert_eq!(feed.key(key(RIGHT_CTRL, false, at(t0, 800))), start(3));
}

#[test]
fn idle_wait_preserves_pending_release_and_click_deadlines() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    assert_eq!(feed.wake_interval(t0), IDLE_WAKE);
    feed.key(key(RIGHT_CTRL, true, t0));
    assert_eq!(feed.wake_interval(t0), WAKE);
    feed.tick(at(t0, 250));
    // TooLong stops recording but a lost key-up must still be polled quickly.
    feed.tick(at(t0, 60_000));
    assert_eq!(feed.wake_interval(at(t0, 60_000)), WAKE);
    feed.poll(false, at(t0, 60_033));
    assert_eq!(feed.wake_interval(at(t0, 60_033)), IDLE_WAKE);
    feed.click(at(t0, 60_100));
    assert_eq!(feed.wake_interval(at(t0, 60_100)), WAKE);
}

const LEFT_CTRL: u32 = mm_core::ptt::LEFT_CTRL;

#[test]
fn either_ctrl_waits_300_ms_alone_and_ends_when_both_sides_are_up() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    feed.set_key(EITHER_CTRL);
    assert!(feed.is_talk(LEFT_CTRL) && feed.is_talk(RIGHT_CTRL));
    assert!(!feed.is_talk(A));
    assert_eq!(feed.key(key(LEFT_CTRL, true, t0)), nothing());
    assert_eq!(feed.tick(at(t0, 299)), nothing());
    assert_eq!(feed.tick(at(t0, 300)), start(1));
    assert_eq!(feed.key(key(RIGHT_CTRL, true, at(t0, 350))), nothing());
    assert_eq!(feed.pressed_at, Some(t0));
    assert_eq!(feed.key(key(LEFT_CTRL, false, at(t0, 400))), nothing());
    assert!(feed.active());
    assert_eq!(
        feed.key(key(RIGHT_CTRL, false, at(t0, 450))),
        stop(1, StopReason::Released)
    );
    // Ctrl+C within the hold starts nothing; the release poll ends the press.
    feed.key(key(LEFT_CTRL, true, at(t0, 500)));
    assert_eq!(feed.key(key(A, true, at(t0, 550))), nothing());
    assert_eq!(feed.tick(at(t0, 1_000)), nothing());
    assert_eq!(feed.poll(false, at(t0, 1_033)), nothing());
    assert!(!feed.needs_poll());
}

#[test]
fn either_ctrl_toggles_on_a_lone_tap_under_400_ms() {
    let t0 = Instant::now();
    let mut feed = toggle(t0);
    feed.set_key(EITHER_CTRL);
    feed.key(key(LEFT_CTRL, true, t0));
    assert_eq!(feed.key(key(LEFT_CTRL, false, at(t0, 399))), start(1));
    feed.key(key(RIGHT_CTRL, true, at(t0, 500)));
    assert_eq!(
        feed.key(key(RIGHT_CTRL, false, at(t0, 600))),
        stop(1, StopReason::Toggled)
    );
    // Ctrl+V is no tap.
    feed.key(key(LEFT_CTRL, true, at(t0, 700)));
    feed.key(key(A, true, at(t0, 750)));
    assert_eq!(feed.key(key(LEFT_CTRL, false, at(t0, 800))), nothing());
    assert!(!feed.active());
}

#[test]
fn released_toggle_uses_idle_wait_without_delaying_its_deadline() {
    let t0 = Instant::now();
    let mut feed = toggle(t0);
    feed.key(key(RIGHT_CTRL, true, t0));
    feed.key(key(RIGHT_CTRL, false, at(t0, 100)));
    assert!(feed.active());
    assert!(!feed.needs_poll());
    assert_eq!(feed.wake_interval(at(t0, 100)), IDLE_WAKE);
    assert_eq!(
        feed.wake_interval(at(t0, 30_090)),
        Duration::from_millis(10)
    );
    assert_eq!(feed.tick(at(t0, 30_100)), stop(1, StopReason::TooLong));
}

#[test]
fn either_ctrl_slow_shortcut_secure_desktop_and_key_switch() {
    let t0 = Instant::now();
    let mut feed = Feed::new(PttSettings::default(), t0);
    feed.set_key(EITHER_CTRL);
    // Ctrl held 1 s, then a letter (Ctrl+C): listening starts at 300 ms, the letter
    // cancels it, and nothing restarts while Ctrl stays down.
    assert_eq!(feed.key(key(LEFT_CTRL, true, t0)), nothing());
    assert_eq!(feed.tick(at(t0, 300)), start(1));
    assert_eq!(feed.key(key(LEFT_CTRL, true, at(t0, 800))), nothing());
    assert_eq!(
        feed.key(key(A, true, at(t0, 1_000))),
        vec![PttEvent::Cancel { generation: 1 }]
    );
    assert_eq!(feed.key(key(A, false, at(t0, 1_100))), nothing());
    assert_eq!(feed.tick(at(t0, 2_000)), nothing());
    assert_eq!(feed.key(key(LEFT_CTRL, false, at(t0, 2_100))), nothing());
    // UAC or the lock screen: the key-up never arrives and the async state reads up.
    feed.key(key(RIGHT_CTRL, true, at(t0, 3_000)));
    assert_eq!(feed.tick(at(t0, 3_300)), start(2));
    assert_eq!(
        feed.poll(false, at(t0, 3_400)),
        stop(2, StopReason::Watchdog)
    );
    assert!(!feed.needs_poll());
    assert_eq!(feed.tick(at(t0, 5_000)), nothing());
    // A key switch while Ctrl is held ends listening; Ctrl's key-up is then any key's.
    feed.key(key(LEFT_CTRL, true, at(t0, 6_000)));
    assert_eq!(feed.tick(at(t0, 6_300)), start(3));
    assert_eq!(feed.set_key(0x78), stop(3, StopReason::Cancelled));
    assert_eq!(feed.key(key(LEFT_CTRL, false, at(t0, 6_400))), nothing());
    assert!(!feed.needs_poll());
}
