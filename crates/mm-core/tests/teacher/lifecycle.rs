use super::*;

#[test]
fn recorded_happy_path_settles_then_finishes() {
    let recording: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/replay/synthetic-text-size/probe-20261026T183422420739Z-meta.json"
    ))
    .unwrap();
    let parsed = parse_response_value(&recording["response"]).unwrap();
    let ids: Vec<_> = parsed.computer_calls[0]
        .safety_checks
        .iter()
        .map(|s| s.id.clone())
        .collect();
    let commentary = parsed.commentary[0].clone();
    let target = PhysRect::new(960, 255, 1160, 287).unwrap();
    let mut teacher = Teacher::new(TeacherConfig::default());
    assert_eq!(
        teacher.input(Input::ModelTurn {
            response: parsed,
            // The recorded turn was sent as an unscaled 1280x800 screenshot.
            frame: FrameTransform::new(PhysRect::new(0, 0, 1280, 800).unwrap(), 1280, 800).unwrap(),
            targets: vec![Some(target)],
        }),
        vec![
            Command::SpeakCommentary(commentary),
            Command::Say(LineKey::StepN),
            Command::PointAt {
                rect: target,
                step: 1,
                total: 1
            }
        ]
    );
    assert_eq!(
        teacher.input(Input::Click {
            at: target.center(),
            left: true,
            injected: false,
            now_ms: 100
        }),
        vec![
            Command::AcknowledgeSafetyChecks(ids),
            Command::Say(LineKey::RightClick)
        ]
    );
    assert!(teacher.input(Input::Tick { now_ms: 699 }).is_empty());
    assert_eq!(
        teacher.input(Input::Tick { now_ms: 700 }),
        vec![Command::RequestScreenshot]
    );
    assert!(teacher.input(Input::Tick { now_ms: 1000 }).is_empty());
    let mut done = response(vec![]);
    done.computer_calls.clear();
    assert_eq!(
        teacher.input(Input::ModelTurn {
            response: done,
            frame: frame(),
            targets: vec![]
        }),
        vec![Command::Finished, Command::Say(LineKey::Done)]
    );
    assert!(teacher.input(click(110, 2000)).is_empty());
}

#[test]
fn near_and_far_clicks_repoint_and_only_far_counts_as_failure() {
    let mut teacher = Teacher::new(TeacherConfig {
        auto_escalate: true,
        ..Default::default()
    });
    teacher.input(turn(vec![click_action()]));
    let repoint = vec![
        Command::Say(LineKey::WrongPlace),
        Command::PointAt {
            rect: rect(),
            step: 1,
            total: 1,
        },
    ];
    assert_eq!(teacher.input(click(150, 1)), repoint);
    assert_eq!(teacher.input(click(150, 2)), repoint);
    assert_eq!(teacher.input(click(200, 3)), repoint);
    let out = teacher.input(click(200, 4));
    assert!(out.contains(&Command::UseModel(ModelChoice::Thorough)));
    assert!(!ack(&out));
    let out = teacher.input(click(128, 5));
    assert!(ack(&out));
    assert!(out.contains(&Command::UseModel(ModelChoice::Quick)));
}

#[test]
fn escalation_is_opt_in_and_manual_thorough_never_escalates() {
    for (configured_model, auto_escalate) in [
        (ModelChoice::Quick, false),
        (ModelChoice::Thorough, false),
        (ModelChoice::Thorough, true),
    ] {
        let mut teacher = Teacher::new(TeacherConfig {
            configured_model,
            auto_escalate,
            ..Default::default()
        });
        teacher.input(turn(vec![click_action()]));
        for now_ms in 0..4 {
            assert!(
                !teacher
                    .input(click(200, now_ms))
                    .iter()
                    .any(|c| matches!(c, Command::UseModel(_)))
            );
        }
    }
}

#[test]
fn wonder_unavailable_falls_back_for_the_rest_of_task() {
    for configured_model in [ModelChoice::Quick, ModelChoice::Thorough] {
        let mut teacher = Teacher::new(TeacherConfig {
            configured_model,
            auto_escalate: true,
            ..Default::default()
        });
        assert_eq!(
            teacher.input(Input::WonderUnavailable),
            vec![Command::UseModel(ModelChoice::Quick)]
        );
        for _ in 0..2 {
            teacher.input(turn(vec![click_action()]));
            for now_ms in 1..4 {
                assert!(
                    !teacher
                        .input(click(200, now_ms))
                        .contains(&Command::UseModel(ModelChoice::Thorough))
                );
            }
            assert!(
                !teacher
                    .input(click(110, 4))
                    .contains(&Command::UseModel(ModelChoice::Thorough))
            );
        }
    }
}

#[test]
fn double_click_and_key_chord_complete_in_order() {
    let mut teacher = Teacher::new(TeacherConfig {
        keyboard_steps: true,
        ..Default::default()
    });
    teacher.input(turn(vec![
        Action::DoubleClick { x: 150, y: 100 },
        Action::Keypress {
            keys: vec!["CTRL".into(), "S".into()],
        },
    ]));
    assert!(teacher.input(click(110, 1)).is_empty());
    assert!(!ack(&teacher.input(Input::DoubleClick {
        at: rect().center(),
        left: true,
        injected: false,
        now_ms: 2
    })));
    assert!(
        teacher
            .input(Input::KeyChord {
                foreground: None,
                keys: vec!["S".into()],
                injected: false,
                now_ms: 3
            })
            .is_empty()
    );
    assert_eq!(
        teacher.input(Input::KeyChord {
            foreground: None,
            keys: vec!["s".into(), "ctrl".into()],
            injected: false,
            now_ms: 4
        }),
        vec![Command::AcknowledgeSafetyChecks(vec!["confirm".into()])]
    );
}

#[test]
fn typed_value_or_next_completes_without_acknowledging_checks() {
    for event in [
        Input::ValueChanged {
            text: "prefix hello suffix".into(),
            now_ms: 10,
        },
        Input::UserSaidNext { now_ms: 10 },
    ] {
        let mut teacher = Teacher::new(TeacherConfig {
            keyboard_steps: true,
            ..Default::default()
        });
        teacher.input(turn(vec![Action::Type {
            text: "hello".into(),
        }]));
        assert!(
            teacher
                .input(Input::ValueChanged {
                    text: "hell".into(),
                    now_ms: 1
                })
                .is_empty()
        );
        assert!(teacher.input(event).is_empty());
        assert_eq!(
            teacher.input(Input::Tick { now_ms: 610 }),
            vec![Command::RequestScreenshot]
        );
    }
}

#[test]
fn unknown_missing_and_invalid_targets_refuse_before_pointing() {
    for (action, target) in [
        (
            Action::Unknown {
                kind: "surprise".into(),
            },
            Some(rect()),
        ),
        (click_action(), None),
        (
            click_action(),
            Some(PhysRect {
                left: 2,
                top: 0,
                right: 1,
                bottom: 1,
            }),
        ),
    ] {
        let mut teacher = Teacher::new(TeacherConfig::default());
        assert_eq!(
            teacher.input(Input::ModelTurn {
                response: response(vec![click_action(), action]),
                frame: frame(),
                targets: vec![Some(rect()), target],
            }),
            vec![
                Command::Refused(RefusalReason::TargetNotFound),
                Command::Say(LineKey::CantFind)
            ]
        );
        assert!(teacher.input(click(110, 1)).is_empty());
    }
    let mut teacher = Teacher::new(TeacherConfig::default());
    assert!(
        teacher
            .input(Input::ModelTurn {
                response: response(vec![click_action()]),
                frame: frame(),
                targets: vec![]
            })
            .contains(&Command::Refused(RefusalReason::TargetNotFound))
    );
}

#[test]
fn budget_allows_twelve_turns_and_stops_at_thirteen() {
    let mut teacher = Teacher::new(TeacherConfig::default());
    for _ in 0..12 {
        assert!(
            !teacher
                .input(turn(vec![click_action()]))
                .contains(&Command::Refused(RefusalReason::TooManyTurns))
        );
    }
    assert_eq!(
        teacher.input(turn(vec![click_action()])),
        vec![
            Command::Refused(RefusalReason::TooManyTurns),
            Command::Say(LineKey::TooManyTurns)
        ]
    );
}

#[test]
fn passive_steps_and_speech_never_acknowledge_safety_checks() {
    let mut teacher = Teacher::new(TeacherConfig::default());
    assert!(
        teacher
            .input(turn(vec![
                Action::Wait,
                Action::Screenshot,
                Action::Move { x: 0, y: 0 }
            ]))
            .is_empty()
    );
    assert_eq!(
        teacher.input(Input::Tick { now_ms: 600 }),
        vec![Command::RequestScreenshot]
    );
    teacher.input(turn(vec![click_action()]));
    assert!(
        teacher
            .input(Input::UserSaidNext { now_ms: 601 })
            .is_empty()
    );
    assert!(
        teacher
            .input(Input::KeyChord {
                foreground: None,
                keys: vec!["Enter".into()],
                injected: false,
                now_ms: 602
            })
            .is_empty()
    );
    assert!(ack(&teacher.input(click(110, 603))));
    assert!(teacher.input(click(110, 604)).is_empty());
}

#[test]
fn checks_cover_the_whole_call_and_trailing_passive_steps() {
    let mut teacher = Teacher::new(TeacherConfig {
        keyboard_steps: true,
        ..Default::default()
    });
    teacher.input(turn(vec![click_action(), click_action(), Action::Wait]));
    assert!(!ack(&teacher.input(click(110, 1))));
    assert!(ack(&teacher.input(click(110, 2))));
    assert_eq!(
        teacher.input(Input::Tick { now_ms: 602 }),
        vec![Command::RequestScreenshot]
    );

    teacher.input(turn(vec![
        Action::Type {
            text: "hello".into(),
        },
        click_action(),
    ]));
    teacher.input(Input::UserSaidNext { now_ms: 603 });
    assert!(!ack(&teacher.input(click(110, 604))));
}

#[test]
fn failures_reset_on_a_new_step_and_backward_times_do_not_end_settle() {
    let mut teacher = Teacher::new(TeacherConfig {
        auto_escalate: true,
        ..Default::default()
    });
    teacher.input(turn(vec![click_action(), click_action()]));
    teacher.input(click(200, 100));
    teacher.input(click(110, 200));
    assert!(
        !teacher
            .input(click(200, 300))
            .contains(&Command::UseModel(ModelChoice::Thorough))
    );
    teacher.input(click(110, 400));
    assert!(teacher.input(Input::Tick { now_ms: 0 }).is_empty());
    assert_eq!(
        teacher.input(Input::Tick { now_ms: 1000 }),
        vec![Command::RequestScreenshot]
    );
}

#[test]
fn unexpected_change_discards_batch_and_requests_immediate_capture() {
    let mut teacher = Teacher::new(TeacherConfig::default());
    teacher.input(turn(vec![click_action(), click_action()]));
    assert_eq!(
        teacher.input(Input::ScreenChangedUnexpectedly { now_ms: 5 }),
        vec![Command::RequestScreenshot]
    );
    assert!(teacher.input(click(110, 6)).is_empty());
    assert!(teacher.input(Input::Tick { now_ms: 1000 }).is_empty());
}

#[test]
fn network_errors_must_be_consecutive_and_refusal_is_terminal() {
    let mut teacher = Teacher::new(TeacherConfig::default());
    assert!(teacher.input(Input::ModelFailed { now_ms: 0 }).is_empty());
    teacher.input(turn(vec![click_action()]));
    assert!(teacher.input(Input::ModelFailed { now_ms: 1 }).is_empty());
    assert_eq!(
        teacher.input(Input::ModelFailed { now_ms: 2 }),
        vec![
            Command::Refused(RefusalReason::ModelUnavailable),
            Command::Say(LineKey::NetworkError)
        ]
    );
    assert!(teacher.input(click(110, 3)).is_empty());
}

#[test]
fn unverifiable_gestures_fail_closed() {
    for action in [
        Action::Scroll {
            x: 0,
            y: 0,
            scroll_x: 0,
            scroll_y: 1,
        },
        Action::Click {
            x: 0,
            y: 0,
            button: "right".into(),
        },
    ] {
        let mut teacher = Teacher::new(TeacherConfig::default());
        assert_eq!(
            teacher.input(turn(vec![action])),
            vec![
                Command::Refused(RefusalReason::UnsupportedAction),
                Command::Say(LineKey::WontPress)
            ]
        );
    }
}

proptest! {
    #[test]
    fn random_input_sequences_never_panic(
        events in prop::collection::vec((0u8..19, any::<i32>(), any::<u64>()), 0..200),
        max_turns in any::<u8>(), auto_escalate in any::<bool>(),
        do_it_for_me in any::<bool>(),
    ) {
        let mut teacher = Teacher::new(TeacherConfig { max_turns, auto_escalate, do_it_for_me, ..Default::default() });
        for (kind, x, now_ms) in events {
            let input = match kind {
                0 => turn(vec![click_action()]),
                1 => click(x, now_ms),
                2 => Input::DoubleClick { at: PhysPoint { x, y: x }, left: x % 2 == 0, injected: x % 3 == 0, now_ms },
                3 => Input::KeyChord { foreground: None, keys: vec![x.to_string()], injected: x % 5 == 0, now_ms },
                4 => Input::ValueChanged { text: x.to_string(), now_ms },
                5 => Input::UserSaidNext { now_ms },
                6 => Input::ScreenChangedUnexpectedly { now_ms },
                7 => Input::Tick { now_ms },
                8 => Input::ModelFailed { now_ms },
                9 => Input::WonderUnavailable,
                10 => turn(vec![Action::Wait, click_action()]),
                11 => turn(vec![Action::Type { text: x.to_string() }]),
                12 => turn(vec![Action::Unknown { kind: x.to_string() }]),
                13 => turn(vec![Action::DoubleClick { x: i64::from(x), y: 0 }]),
                15 => Input::DoItRequested { now_ms },
                16 => Input::DoItAnswer { yes: x % 2 == 0, now_ms },
                17 => Input::DoItUnavailable { now_ms },
                18 => Input::DoItDone { ok: x % 2 == 0, now_ms },
                _ => turn(vec![Action::Keypress { keys: vec![x.to_string()] }]),
            };
            teacher.input(input);
        }
    }
}
#[test]
fn a_cut_off_turn_is_a_model_failure_not_done() {
    let mut teacher = Teacher::new(TeacherConfig::default());
    let mut cut = response(vec![]);
    cut.computer_calls.clear();
    cut.status = "incomplete".into();
    cut.incomplete_reason = Some("max_output_tokens".into());
    let turn = |response: ParsedResponse| Input::ModelTurn {
        response,
        frame: frame(),
        targets: vec![],
    };
    assert!(teacher.input(turn(cut.clone())).is_empty());
    assert_eq!(
        teacher.input(turn(cut)),
        vec![
            Command::Refused(RefusalReason::ModelUnavailable),
            Command::Say(LineKey::NetworkError)
        ]
    );
}

#[test]
fn disabled_keyboard_steps_refuse_before_commentary_or_pointing() {
    assert!(!TeacherConfig::default().keyboard_steps);
    for action in [
        Action::Type {
            text: "hello".into(),
        },
        Action::Keypress {
            keys: vec!["Enter".into()],
        },
    ] {
        let mut teacher = Teacher::new(TeacherConfig::default());
        let mut response = response(vec![click_action(), action]);
        response.commentary.push("Click here first".into());
        assert_eq!(
            teacher.input(Input::ModelTurn {
                response,
                frame: frame(),
                targets: vec![Some(rect()), None],
            }),
            vec![
                Command::Refused(RefusalReason::UnsupportedAction),
                Command::Say(LineKey::WontPress),
            ]
        );
        assert!(teacher.input(click(110, 1)).is_empty());
    }
}

#[test]
fn turn_budget_refuses_at_settle_before_requesting_an_extra_turn() {
    for passive in [false, true] {
        let mut teacher = Teacher::new(TeacherConfig {
            max_turns: 2,
            ..Default::default()
        });
        for turn_index in 0..2 {
            let start = turn_index * 600;
            teacher.input(turn(vec![if passive {
                Action::Wait
            } else {
                click_action()
            }]));
            if !passive {
                assert!(ack(&teacher.input(click(110, start))));
            }
            assert!(
                teacher
                    .input(Input::Tick {
                        now_ms: start + 599
                    })
                    .is_empty()
            );
            assert_eq!(
                teacher.input(Input::Tick {
                    now_ms: start + 600
                }),
                if turn_index == 0 {
                    vec![Command::RequestScreenshot]
                } else {
                    vec![
                        Command::Refused(RefusalReason::TooManyTurns),
                        Command::Say(LineKey::TooManyTurns),
                    ]
                }
            );
        }
        assert!(teacher.input(Input::Tick { now_ms: 2000 }).is_empty());
        assert!(teacher.input(turn(vec![click_action()])).is_empty());
    }
}

#[test]
fn debug_hides_typed_values_keys_and_spoken_lines() {
    let typed = format!(
        "{:?}",
        Input::ValueChanged {
            text: "hunter2".into(),
            now_ms: 1
        }
    );
    let keys = format!(
        "{:?}",
        Input::KeyChord {
            foreground: None,
            keys: vec!["secret".into()],
            injected: false,
            now_ms: 1
        }
    );
    let spoken = format!("{:?}", Command::SpeakCommentary("my bank pin".into()));
    for text in [&typed, &keys, &spoken] {
        assert!(!text.contains("hunter2") && !text.contains("secret") && !text.contains("pin"));
    }
}

#[test]
fn key_match_for_focus_hint_requires_the_active_human_chord() {
    let mut teacher = Teacher::new(TeacherConfig {
        keyboard_steps: true,
        ..TeacherConfig::default()
    });
    let chord = Input::KeyChord {
        foreground: None,
        keys: vec!["control".into(), "+".into()],
        injected: false,
        now_ms: 10,
    };
    assert!(!teacher.matches_key_chord(&chord));
    teacher.input(turn(vec![Action::Keypress {
        keys: vec!["CTRL".into(), "plus".into()],
    }]));
    assert!(teacher.matches_key_chord(&chord));
    assert!(teacher.matches_key_chord(&chord)); // Matching never consumes the step.
    assert!(!teacher.matches_key_chord(&Input::KeyChord {
        foreground: None,
        keys: vec!["ctrl".into(), "plus".into()],
        injected: true,
        now_ms: 10,
    }));
    assert!(!teacher.matches_key_chord(&Input::KeyChord {
        foreground: None,
        keys: vec!["esc".into()],
        injected: false,
        now_ms: 10,
    }));
    teacher.input(chord.clone());
    assert!(!teacher.matches_key_chord(&chord));
}
