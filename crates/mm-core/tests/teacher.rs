use mm_core::api::response::{
    Action, ComputerCall, ParsedResponse, SafetyCheck, parse_response_value,
};
use mm_core::geom::{FrameTransform, PhysPoint, PhysRect};
use mm_core::teacher::{
    LineKey, ModelChoice, RefusalReason, Teacher, TeacherCommand as Command, TeacherConfig,
    TeacherInput as Input,
};
use proptest::prelude::*;

fn rect() -> PhysRect {
    PhysRect::new(100, 100, 120, 120).unwrap()
}

fn frame() -> FrameTransform {
    FrameTransform::new(PhysRect::new(-200, -100, 1280, 800).unwrap(), 740, 450).unwrap()
}

fn response(actions: Vec<Action>) -> ParsedResponse {
    ParsedResponse {
        id: "turn".into(),
        model: "muse-spark-1.3".into(),
        status: "completed".into(),
        commentary: vec![],
        computer_calls: vec![ComputerCall {
            call_id: "call".into(),
            actions,
            safety_checks: vec![SafetyCheck {
                id: "confirm".into(),
                code: "computer_action_requires_confirmation".into(),
                message: "Confirm".into(),
            }],
            replay_receipt: None,
        }],
        usage: None,
        incomplete_reason: None,
        reasoning_items: 0,
    }
}

fn turn(actions: Vec<Action>) -> Input {
    Input::ModelTurn {
        targets: vec![Some(rect()); actions.len()],
        response: response(actions),
        frame: frame(),
    }
}

fn click_action() -> Action {
    Action::Click {
        x: 150,
        y: 100,
        button: "left".into(),
    }
}

fn click(x: i32, now_ms: u64) -> Input {
    Input::Click {
        at: PhysPoint { x, y: 110 },
        left: true,
        injected: false,
        now_ms,
    }
}

fn ack(out: &[Command]) -> bool {
    out.iter()
        .any(|cmd| matches!(cmd, Command::AcknowledgeSafetyChecks(_)))
}

#[test]
fn recorded_happy_path_settles_then_finishes() {
    let recording: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/replay/synthetic-text-size/probe-20261026T183422420739Z-meta.json"
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
                keys: vec!["S".into()],
                injected: false,
                now_ms: 3
            })
            .is_empty()
    );
    assert_eq!(
        teacher.input(Input::KeyChord {
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
        events in prop::collection::vec((0u8..15, any::<i32>(), any::<u64>()), 0..200),
        max_turns in any::<u8>(), auto_escalate in any::<bool>(),
    ) {
        let mut teacher = Teacher::new(TeacherConfig { max_turns, auto_escalate, ..Default::default() });
        for (kind, x, now_ms) in events {
            let input = match kind {
                0 => turn(vec![click_action()]),
                1 => click(x, now_ms),
                2 => Input::DoubleClick { at: PhysPoint { x, y: x }, left: x % 2 == 0, injected: x % 3 == 0, now_ms },
                3 => Input::KeyChord { keys: vec![x.to_string()], injected: x % 5 == 0, now_ms },
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
                _ => turn(vec![Action::Keypress { keys: vec![x.to_string()] }]),
            };
            teacher.input(input);
        }
    }
}

// Findings of the 2026-10-27 safety review (Claude worker #460).

fn click_at(x: i64, y: i64) -> Action {
    Action::Click {
        x,
        y,
        button: "left".into(),
    }
}

#[test]
fn model_points_off_the_screenshot_or_far_from_the_target_are_refused() {
    // frame(): 740x450 sent; the target rect maps from about (150, 100).
    for action in [
        click_at(-40, 100),
        click_at(99_999, 100),
        click_at(150, 450),
        click_at(400, 300),
    ] {
        let mut teacher = Teacher::new(TeacherConfig::default());
        let mut turn = turn(vec![action]);
        if let Input::ModelTurn { response, .. } = &mut turn {
            response.commentary = vec!["Click the button below".into()];
        }
        let out = teacher.input(turn);
        assert_eq!(
            out,
            vec![
                Command::Refused(RefusalReason::TargetNotFound),
                Command::Say(LineKey::CantFind)
            ],
            "the spoken line must not play before a refusal"
        );
    }
}

#[test]
fn a_target_outside_the_capture_is_refused() {
    let mut teacher = Teacher::new(TeacherConfig::default());
    let outside = PhysRect::new(5000, 5000, 5020, 5020).unwrap();
    let out = teacher.input(Input::ModelTurn {
        response: response(vec![click_action()]),
        frame: frame(),
        targets: vec![Some(outside)],
    });
    assert!(out.contains(&Command::Refused(RefusalReason::TargetNotFound)));
}

#[test]
fn right_and_injected_clicks_and_keys_never_complete_a_step() {
    let mut teacher = Teacher::new(TeacherConfig::default());
    teacher.input(turn(vec![click_action()]));
    let at = rect().center();
    for (left, injected) in [(false, false), (true, true), (false, true)] {
        let out = teacher.input(Input::Click {
            at,
            left,
            injected,
            now_ms: 1,
        });
        assert!(out.is_empty(), "left={left} injected={injected}");
    }
    let mut keys = Teacher::new(TeacherConfig {
        keyboard_steps: true,
        ..Default::default()
    });
    keys.input(turn(vec![Action::Keypress {
        keys: vec!["Enter".into()],
    }]));
    let injected = keys.input(Input::KeyChord {
        keys: vec!["Enter".into()],
        injected: true,
        now_ms: 1,
    });
    assert!(injected.is_empty());
    assert!(ack(&teacher.input(click(110, 2))));
}

fn drag_action() -> Action {
    // The first point maps to (100, 100); the end is outside the target.
    Action::Drag {
        path: vec![(150, 100), (200, 100)],
    }
}

#[test]
fn drag_points_at_slider_and_human_release_confirms_safety_checks() {
    // Includes points outside hit tolerance and a corner of the inflated rect.
    for at in [
        rect().center(),
        PhysPoint { x: 159, y: 110 },
        PhysPoint { x: 60, y: 60 },
        PhysPoint { x: 159, y: 159 },
    ] {
        let mut teacher = Teacher::new(TeacherConfig::default());
        assert_eq!(
            teacher.input(turn(vec![drag_action()])),
            vec![
                Command::Say(LineKey::StepN),
                Command::PointAt {
                    rect: rect(),
                    step: 1,
                    total: 1,
                }
            ]
        );
        assert_eq!(
            teacher.input(Input::Click {
                at,
                left: true,
                injected: false,
                now_ms: 10,
            }),
            vec![
                Command::AcknowledgeSafetyChecks(vec!["confirm".into()]),
                Command::Say(LineKey::RightClick),
            ]
        );
        assert!(teacher.input(Input::Tick { now_ms: 609 }).is_empty());
        assert_eq!(
            teacher.input(Input::Tick { now_ms: 610 }),
            vec![Command::RequestScreenshot]
        );
    }
}

#[test]
fn drag_ignores_injected_and_non_left_releases() {
    let mut teacher = Teacher::new(TeacherConfig::default());
    teacher.input(turn(vec![drag_action()]));
    for (left, injected) in [(true, true), (false, false), (false, true)] {
        assert!(
            teacher
                .input(Input::Click {
                    at: rect().center(),
                    left,
                    injected,
                    now_ms: 1,
                })
                .is_empty()
        );
    }
    assert!(teacher.input(Input::UserSaidNext { now_ms: 2 }).is_empty());
    assert!(teacher.input(Input::Tick { now_ms: 1000 }).is_empty());
    assert!(ack(&teacher.input(click(110, 1001))));
}

#[test]
fn far_drag_releases_repoint_and_count_as_failed_attempts() {
    for at in [
        PhysPoint { x: 250, y: 110 },
        PhysPoint { x: 59, y: 110 },
        PhysPoint { x: 160, y: 140 },
        PhysPoint { x: 110, y: 59 },
        PhysPoint { x: 110, y: 160 },
    ] {
        let mut teacher = Teacher::new(TeacherConfig {
            auto_escalate: true,
            ..Default::default()
        });
        teacher.input(turn(vec![drag_action()]));
        let release = |now_ms| Input::Click {
            at,
            left: true,
            injected: false,
            now_ms,
        };
        let repoint = Command::PointAt {
            rect: rect(),
            step: 1,
            total: 1,
        };
        assert_eq!(
            teacher.input(release(1)),
            vec![Command::Say(LineKey::WrongPlace), repoint.clone()]
        );
        assert_eq!(
            teacher.input(release(2)),
            vec![
                Command::Say(LineKey::WrongPlace),
                Command::UseModel(ModelChoice::Thorough),
                repoint,
            ]
        );
        assert_eq!(
            teacher.input(click(150, 3)),
            vec![
                Command::AcknowledgeSafetyChecks(vec!["confirm".into()]),
                Command::UseModel(ModelChoice::Quick),
                Command::Say(LineKey::RightClick),
            ]
        );
    }
}

#[test]
fn short_or_implausible_drag_starts_are_refused() {
    for path in [
        vec![],
        vec![(150, 100)],
        vec![(-1, 100), (150, 100)],
        vec![(740, 100), (150, 100)],
        vec![(150, 450), (150, 100)],
        vec![(400, 300), (150, 100)],
        vec![(150, 100), (-1, 100)],
        vec![(150, 100), (740, 100)],
        vec![(150, 100), (200, 450)],
        vec![(150, 100), (200, -1)],
    ] {
        let mut teacher = Teacher::new(TeacherConfig::default());
        assert_eq!(
            teacher.input(turn(vec![Action::Drag { path }])),
            vec![
                Command::Refused(RefusalReason::TargetNotFound),
                Command::Say(LineKey::CantFind),
            ]
        );
        assert!(teacher.input(click(110, 1)).is_empty());
    }
}

#[test]
fn drag_then_click_runs_in_order_and_checks_cover_both() {
    let mut teacher = Teacher::new(TeacherConfig::default());
    teacher.input(turn(vec![drag_action(), click_action()]));
    // This release is inside the drag's inflated rect, beyond click tolerance.
    assert_eq!(
        teacher.input(click(150, 1)),
        vec![
            Command::Say(LineKey::RightClick),
            Command::Say(LineKey::StepN),
            Command::PointAt {
                rect: rect(),
                step: 2,
                total: 2,
            },
        ]
    );
    assert!(
        teacher
            .input(click(150, 2))
            .contains(&Command::Say(LineKey::WrongPlace))
    );
    assert!(ack(&teacher.input(click(110, 3))));
    assert_eq!(
        teacher.input(Input::Tick { now_ms: 603 }),
        vec![Command::RequestScreenshot]
    );
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
fn drag_release_near_mapped_endpoint_completes() {
    // frame() scales by two and has a negative origin: (400, 200) -> (600, 300).
    for at in [
        PhysPoint { x: 600, y: 300 },
        PhysPoint { x: 624, y: 332 }, // Exactly near_px from the endpoint.
    ] {
        let mut teacher = Teacher::new(TeacherConfig::default());
        teacher.input(turn(vec![Action::Drag {
            path: vec![(150, 100), (400, 200)],
        }]));
        for (left, injected) in [(true, true), (false, false)] {
            assert!(
                teacher
                    .input(Input::Click {
                        at,
                        left,
                        injected,
                        now_ms: 1,
                    })
                    .is_empty()
            );
        }
        // A screenshot-space release and a point just beyond the endpoint radius miss.
        for wrong in [PhysPoint { x: 400, y: 200 }, PhysPoint { x: 625, y: 332 }] {
            let out = teacher.input(Input::Click {
                at: wrong,
                left: true,
                injected: false,
                now_ms: 2,
            });
            assert!(out.contains(&Command::Say(LineKey::WrongPlace)));
            assert!(!ack(&out));
        }
        assert_eq!(
            teacher.input(Input::Click {
                at,
                left: true,
                injected: false,
                now_ms: 3,
            }),
            vec![
                Command::AcknowledgeSafetyChecks(vec!["confirm".into()]),
                Command::Say(LineKey::RightClick),
            ]
        );
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
