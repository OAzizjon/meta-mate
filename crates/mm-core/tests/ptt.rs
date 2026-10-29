use mm_core::ptt::{Key, Mode, Ptt, PttEvent, PttInput, PttSettings, StopReason};
use proptest::prelude::*;

fn down(key: Key, now_ms: u64) -> PttInput {
    PttInput::KeyDown {
        key,
        injected: false,
        now_ms,
    }
}

fn up(key: Key, now_ms: u64) -> PttInput {
    PttInput::KeyUp {
        key,
        injected: false,
        now_ms,
    }
}

fn tick(now_ms: u64) -> PttInput {
    PttInput::Tick { now_ms }
}

fn poll(ptt_key_down: bool, now_ms: u64) -> PttInput {
    PttInput::Poll {
        ptt_key_down,
        now_ms,
    }
}

fn start(generation: u64) -> PttEvent {
    PttEvent::Start { generation }
}

fn stop(generation: u64, reason: StopReason) -> PttEvent {
    PttEvent::Stop { generation, reason }
}

fn timeline(mode: Mode, rows: Vec<(PttInput, Vec<PttEvent>)>) {
    let mut ptt = Ptt::new(PttSettings {
        mode,
        ..PttSettings::default()
    });
    for (input, expected) in rows {
        assert_eq!(ptt.input(input), expected, "input: {input:?}");
    }
}

#[test]
fn hold_threshold_release_and_generations() {
    timeline(
        Mode::Hold,
        vec![
            (down(Key::Talk, 0), vec![]),
            (tick(249), vec![]),
            (tick(250), vec![start(1)]),
            (tick(500), vec![]),
            (up(Key::Talk, 501), vec![stop(1, StopReason::Released)]),
            (down(Key::Talk, 600), vec![]),
            (poll(true, 850), vec![start(2)]),
            (up(Key::Talk, 851), vec![stop(2, StopReason::Released)]),
            (down(Key::Talk, 900), vec![]),
            (up(Key::Talk, 1149), vec![]),
            (tick(2000), vec![]),
        ],
    );
}

#[test]
fn chord_stays_blocked_until_ctrl_release() {
    for chord_at in [100, 250, 500] {
        timeline(
            Mode::Hold,
            vec![
                (down(Key::Talk, 0), vec![]),
                (down(Key::Other(0x43), chord_at), vec![]),
                (up(Key::Other(0x43), chord_at + 1), vec![]),
                (tick(1000), vec![]),
                (up(Key::Talk, 1001), vec![]),
                (down(Key::Talk, 1002), vec![]),
                (tick(1252), vec![start(1)]),
                (
                    down(Key::Other(0x43), 1253),
                    vec![PttEvent::Cancel { generation: 1 }],
                ),
                (tick(2000), vec![]),
                (up(Key::Talk, 2001), vec![]),
            ],
        );
    }
    timeline(
        Mode::Hold,
        vec![
            (down(Key::Other(0x43), 0), vec![]),
            (down(Key::Talk, 1), vec![]),
            (up(Key::Other(0x43), 2), vec![]),
            (tick(1000), vec![]),
        ],
    );
}

#[test]
fn repeats_do_not_reset_hold_or_rearm_after_timeout() {
    timeline(
        Mode::Hold,
        vec![
            (down(Key::Talk, 0), vec![]),
            (down(Key::Talk, 200), vec![]),
            (tick(250), vec![start(1)]),
            (down(Key::Talk, 300), vec![]),
            (tick(60_249), vec![]),
            (tick(60_250), vec![stop(1, StopReason::TooLong)]),
            (down(Key::Talk, 60_251), vec![]),
            (tick(70_000), vec![]),
            (up(Key::Talk, 70_001), vec![]),
            (down(Key::Talk, 70_002), vec![]),
            (tick(70_252), vec![start(2)]),
        ],
    );
}

#[test]
fn injected_events_do_not_change_keys_or_advance_time() {
    let injected = |key, now_ms, is_down| {
        if is_down {
            PttInput::KeyDown {
                key,
                injected: true,
                now_ms,
            }
        } else {
            PttInput::KeyUp {
                key,
                injected: true,
                now_ms,
            }
        }
    };
    timeline(
        Mode::Hold,
        vec![
            (injected(Key::Talk, 100_000, true), vec![]),
            (tick(250), vec![]),
            (down(Key::Talk, 300), vec![]),
            (injected(Key::Other(0x43), 100_000, true), vec![]),
            (injected(Key::Talk, 100_000, false), vec![]),
            (tick(550), vec![start(1)]),
            (injected(Key::Talk, 100_000, false), vec![]),
            (injected(Key::Other(0x43), 100_000, true), vec![]),
            (up(Key::Talk, 551), vec![stop(1, StopReason::Released)]),
        ],
    );
}

#[test]
fn watchdog_recovers_release_and_clears_pending_hold() {
    timeline(
        Mode::Hold,
        vec![
            (poll(true, 0), vec![]),
            (down(Key::Talk, 1), vec![]),
            (poll(false, 251), vec![]),
            (tick(1000), vec![]),
            (down(Key::Talk, 1001), vec![]),
            (poll(true, 1251), vec![start(1)]),
            (poll(false, 1284), vec![stop(1, StopReason::Watchdog)]),
            (up(Key::Talk, 1285), vec![]),
            (poll(false, 1317), vec![]),
            (down(Key::Talk, 1400), vec![]),
            (tick(1650), vec![start(2)]),
        ],
    );
}

#[test]
fn toggle_requires_lone_short_taps_and_ignores_release_poll_while_listening() {
    timeline(
        Mode::Toggle,
        vec![
            (up(Key::Talk, 0), vec![]),
            (down(Key::Talk, 1), vec![]),
            (tick(251), vec![]),
            (up(Key::Talk, 251), vec![start(1)]),
            (poll(false, 252), vec![]),
            (down(Key::Talk, 300), vec![]),
            (down(Key::Talk, 310), vec![]),
            (up(Key::Talk, 320), vec![stop(1, StopReason::Toggled)]),
            (down(Key::Talk, 400), vec![]),
            (up(Key::Talk, 651), vec![]),
            (down(Key::Talk, 700), vec![]),
            (down(Key::Other(0x43), 710), vec![]),
            (up(Key::Other(0x43), 711), vec![]),
            (up(Key::Talk, 720), vec![]),
            (down(Key::Talk, 800), vec![]),
            (poll(false, 810), vec![]),
            (up(Key::Talk, 820), vec![]),
            (down(Key::Talk, 900), vec![]),
            (up(Key::Talk, 901), vec![start(2)]),
            (tick(60_901), vec![stop(2, StopReason::TooLong)]),
        ],
    );
}

#[test]
fn toggle_chord_after_start_drops_audio() {
    timeline(
        Mode::Toggle,
        vec![
            (down(Key::Talk, 0), vec![]),
            (up(Key::Talk, 1), vec![start(1)]),
            (down(Key::Other(0x43), 2), vec![]),
            (up(Key::Other(0x43), 3), vec![]),
            (down(Key::Talk, 4), vec![]),
            (
                down(Key::Other(0x43), 5),
                vec![PttEvent::Cancel { generation: 1 }],
            ),
            (up(Key::Talk, 6), vec![]),
        ],
    );
}

#[test]
fn settings_change_stops_and_requires_fresh_press() {
    for mode in [Mode::Hold, Mode::Toggle] {
        let settings = PttSettings {
            mode,
            hold_ms: 10,
            max_ms: 20,
        };
        timeline(
            Mode::Hold,
            vec![
                (down(Key::Talk, 0), vec![]),
                (tick(250), vec![start(1)]),
                (
                    PttInput::SettingsChanged(settings),
                    vec![stop(1, StopReason::Cancelled)],
                ),
                (tick(1000), vec![]),
                (down(Key::Talk, 1001), vec![]),
                (up(Key::Talk, 1002), vec![]),
                (PttInput::SettingsChanged(PttSettings::default()), vec![]),
                (down(Key::Talk, 1003), vec![]),
                (tick(1253), vec![start(2)]),
            ],
        );
        let mut ptt = Ptt::new(settings);
        ptt.input(down(Key::Talk, 0));
        let began = if mode == Mode::Hold {
            tick(10)
        } else {
            up(Key::Talk, 10)
        };
        assert_eq!(ptt.input(began), vec![start(1)]);
        assert_eq!(ptt.input(tick(29)), vec![]);
        assert_eq!(ptt.input(tick(30)), vec![stop(1, StopReason::TooLong)]);
    }
}

#[test]
fn settings_change_during_toggle_recording_or_pending_hold() {
    timeline(
        Mode::Toggle,
        vec![
            (down(Key::Talk, 0), vec![]),
            (up(Key::Talk, 1), vec![start(1)]),
            (
                PttInput::SettingsChanged(PttSettings::default()),
                vec![stop(1, StopReason::Cancelled)],
            ),
            (poll(false, 2), vec![]),
            (down(Key::Talk, 3), vec![]),
            (PttInput::SettingsChanged(PttSettings::default()), vec![]),
            (tick(253), vec![]),
            (up(Key::Talk, 254), vec![]),
            (down(Key::Talk, 255), vec![]),
            (tick(505), vec![start(2)]),
        ],
    );
}

#[test]
fn extreme_and_backward_times_do_not_overflow() {
    timeline(
        Mode::Hold,
        vec![
            (down(Key::Talk, u64::MAX - 250), vec![]),
            (tick(0), vec![]),
            (tick(u64::MAX), vec![start(1)]),
            (up(Key::Talk, 0), vec![stop(1, StopReason::Released)]),
        ],
    );
    let mut ptt = Ptt::new(PttSettings {
        hold_ms: 0,
        max_ms: 0,
        ..PttSettings::default()
    });
    assert_eq!(ptt.input(down(Key::Talk, 0)), vec![start(1)]);
    assert_eq!(ptt.input(tick(0)), vec![stop(1, StopReason::TooLong)]);
    assert!(ptt.input(tick(u64::MAX)).is_empty());
}

proptest! {
    #[test]
    fn arbitrary_inputs_never_double_start_or_panic(
        inputs in prop::collection::vec((0u8..7, any::<u64>(), any::<u32>(), any::<bool>(), any::<u64>()), 0..500)
    ) {
        let mut ptt = Ptt::default();
        let mut active = None;
        let mut last_generation = 0;
        for (kind, now_ms, code, flag, duration) in inputs {
            let key = if flag { Key::Talk } else { Key::Other(code) };
            let input = match kind {
                0 => down(key, now_ms),
                1 => up(key, now_ms),
                2 => PttInput::KeyDown { key, injected: true, now_ms },
                3 => PttInput::KeyUp { key, injected: true, now_ms },
                4 => poll(flag, now_ms),
                5 => tick(now_ms),
                _ => PttInput::SettingsChanged(PttSettings {
                    mode: if flag { Mode::Hold } else { Mode::Toggle },
                    hold_ms: duration,
                    max_ms: now_ms,
                }),
            };
            for event in ptt.input(input) {
                match event {
                    PttEvent::Start { generation } => {
                        prop_assert!(active.is_none());
                        prop_assert!(generation > last_generation);
                        last_generation = generation;
                        active = Some(generation);
                    }
                    PttEvent::Stop { generation, .. } | PttEvent::Cancel { generation } => {
                        prop_assert_eq!(active.take(), Some(generation));
                    }
                }
            }
        }
    }
}

#[test]
fn a_key_whose_release_was_lost_does_not_block_ptt_forever() {
    timeline(
        Mode::Hold,
        vec![
            // The key-up of 0x43 never arrives (Windows dropped the hook for a moment).
            (down(Key::Other(0x43), 0), vec![]),
            (down(Key::Talk, 5_000), vec![]),
            (tick(5_250), vec![start(1)]),
        ],
    );
}
#[test]
fn configured_virtual_keys_classify_before_chord_detection() {
    use mm_core::ptt::suppress_lock_key;
    for talk_key in [0xA3, 0xA5, 0x14, 0x2D, 0x78, 0x79] {
        assert_eq!(Key::from_virtual_key(talk_key, talk_key), Key::Talk);
        assert_eq!(Key::from_virtual_key(0x41, talk_key), Key::Other(0x41));
        if talk_key != 0xA3 {
            assert_eq!(Key::from_virtual_key(0xA3, talk_key), Key::Other(0xA3));
        }
        assert_eq!(suppress_lock_key(0x14, talk_key, false), talk_key == 0x14);
        assert!(!suppress_lock_key(0x14, talk_key, true));
        assert!(!suppress_lock_key(0x41, talk_key, false));
    }
}
