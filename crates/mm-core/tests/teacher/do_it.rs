use super::*;

fn do_it_teacher(actions: Vec<Action>) -> Teacher {
    let mut teacher = Teacher::new(TeacherConfig {
        do_it_for_me: true,
        keyboard_steps: true,
        ..Default::default()
    });
    teacher.input(do_it_turn(actions));
    teacher
}

fn do_it_turn(actions: Vec<Action>) -> Input {
    let targets = vec![Some(rect()); actions.len()];
    let mut response = response(actions);
    response.computer_calls[0].safety_checks.clear();
    Input::ModelTurn {
        targets,
        response,
        frame: frame(),
    }
}

fn offer() -> Command {
    Command::OfferDoIt { rect: rect() }
}

#[test]
fn delegated_steps_offer_immediately_and_each_click_needs_its_own_yes() {
    let mut teacher = Teacher::new(TeacherConfig {
        do_it_for_me: true,
        delegated: true,
        ..Default::default()
    });
    let out = teacher.input(do_it_turn(vec![
        Action::Wait,
        click_action(),
        click_action(),
    ]));
    assert_eq!(out.last(), Some(&offer()));
    assert!(
        !out.iter()
            .any(|cmd| matches!(cmd, Command::PerformClick { .. }))
    );
    assert!(teacher.input(Input::Tick { now_ms: 1 }).is_empty());
    for now_ms in [2, 4] {
        let out = teacher.input(Input::DoItAnswer { yes: true, now_ms });
        assert_eq!(
            out.iter()
                .filter(|cmd| matches!(cmd, Command::PerformClick { .. }))
                .count(),
            1
        );
        assert!(
            teacher
                .input(Input::DoItAnswer { yes: true, now_ms })
                .is_empty()
        );
        let out = teacher.input(Input::DoItDone {
            ok: true,
            now_ms: now_ms + 1,
        });
        assert!(
            !out.iter()
                .any(|cmd| matches!(cmd, Command::PerformClick { .. }))
        );
        assert!(!ack(&out));
        assert_eq!(out.contains(&offer()), now_ms == 2);
    }
    assert_eq!(
        teacher.input(Input::Tick { now_ms: 605 }),
        vec![Command::RequestScreenshot]
    );
    // Delegation persists across screenshot/model turns, but carries no consent.
    let out = teacher.input(do_it_turn(vec![click_action()]));
    assert_eq!(out.last(), Some(&offer()));
    assert!(
        !out.iter()
            .any(|cmd| matches!(cmd, Command::PerformClick { .. }))
    );
    teacher.input(Input::DoItAnswer {
        yes: false,
        now_ms: 606,
    });
    assert!(
        teacher
            .input(Input::DoItAnswer {
                yes: true,
                now_ms: 607,
            })
            .is_empty()
    );
    assert!(
        !teacher
            .input(Input::Tick { now_ms: 608 })
            .contains(&offer())
    );
}

#[test]
fn delegation_preserves_offer_guards_and_how_still_starts_with_pointing() {
    for (enabled, delegated, checked) in [
        (true, false, false),
        (false, true, false),
        (true, true, true),
    ] {
        let mut teacher = Teacher::new(TeacherConfig {
            do_it_for_me: enabled,
            delegated,
            ..Default::default()
        });
        let out = teacher.input(if checked {
            turn(vec![click_action()])
        } else {
            do_it_turn(vec![click_action()])
        });
        assert!(!out.contains(&offer()));
        assert!(
            teacher
                .input(Input::DoItAnswer {
                    yes: true,
                    now_ms: 1,
                })
                .is_empty()
        );
        if checked {
            assert!(ack(&teacher.input(click(110, 2))));
        }
    }
    for action in [
        Action::DoubleClick { x: 150, y: 100 },
        drag_action(),
        Action::Type {
            text: "hello".into(),
        },
        Action::Keypress {
            keys: vec!["Enter".into()],
        },
        Action::Click {
            x: 150,
            y: 100,
            button: "right".into(),
        },
    ] {
        let mut teacher = Teacher::new(TeacherConfig {
            do_it_for_me: true,
            delegated: true,
            keyboard_steps: true,
            ..Default::default()
        });
        let out = teacher.input(do_it_turn(vec![action]));
        assert!(!out.contains(&offer()));
        assert!(
            !out.iter()
                .any(|cmd| matches!(cmd, Command::PerformClick { .. }))
        );
        assert!(
            teacher
                .input(Input::DoItAnswer {
                    yes: true,
                    now_ms: 1,
                })
                .is_empty()
        );
    }
}

#[test]
fn speech_after_an_offer_cancels_it_before_a_later_yes() {
    for automatic in [false, true] {
        let mut teacher = Teacher::new(TeacherConfig {
            do_it_for_me: true,
            auto_escalate: automatic,
            ..Default::default()
        });
        teacher.input(do_it_turn(vec![click_action()]));
        let commands = if automatic {
            teacher.input(click(200, 1));
            teacher.input(click(200, 2))
        } else {
            assert!(
                teacher
                    .input(Input::DoItRequested { now_ms: 1 })
                    .contains(&offer())
            );
            teacher.input(click(200, 2))
        };
        let mut open = !automatic;
        let mut closed_by_speech = false;
        for command in commands {
            if matches!(command, Command::OfferDoIt { .. }) {
                open = true;
            } else if open && mm_core::teacher::closes_confirmation(&command) {
                open = false;
                closed_by_speech = true;
                let _ = teacher.input(Input::DoItAnswer {
                    yes: false,
                    now_ms: 3,
                });
            }
        }
        assert!(closed_by_speech, "automatic: {automatic}");
        assert!(!open);
        assert!(
            teacher
                .input(Input::DoItAnswer {
                    yes: true,
                    now_ms: 4,
                })
                .is_empty()
        );
    }
}

#[test]
fn pending_safety_checks_never_offer_or_inject_even_after_yes() {
    let mut teacher = Teacher::new(TeacherConfig {
        do_it_for_me: true,
        ..Default::default()
    });
    teacher.input(turn(vec![click_action()]));
    let mut out = teacher.input(Input::DoItRequested { now_ms: 1 });
    out.extend(teacher.input(click(200, 2)));
    out.extend(teacher.input(click(200, 3)));
    out.extend(teacher.input(Input::DoItAnswer {
        yes: true,
        now_ms: 4,
    }));
    assert!(!out.iter().any(|command| matches!(
        command,
        Command::OfferDoIt { .. }
            | Command::PerformClick { .. }
            | Command::AcknowledgeSafetyChecks(_)
    )));
    assert!(ack(&teacher.input(click(110, 5))));
}

#[test]
fn one_yes_performs_only_one_current_click_and_done_uses_human_completion() {
    let mut teacher = do_it_teacher(vec![click_action(), click_action(), Action::Wait]);
    assert!(
        teacher
            .input(Input::DoItAnswer {
                yes: true,
                now_ms: 0
            })
            .is_empty()
    );
    assert_eq!(
        teacher.input(Input::DoItRequested { now_ms: 1 }),
        vec![offer()]
    );
    assert!(teacher.input(Input::DoItRequested { now_ms: 2 }).is_empty());
    assert_eq!(
        teacher.input(Input::DoItAnswer {
            yes: true,
            now_ms: 3
        }),
        vec![
            Command::Say(LineKey::Pressing),
            Command::PerformClick {
                at: rect().center(),
                rect: rect()
            },
        ]
    );
    assert!(
        teacher
            .input(Input::DoItAnswer {
                yes: true,
                now_ms: 4
            })
            .is_empty()
    );
    assert!(teacher.input(Input::DoItRequested { now_ms: 5 }).is_empty());
    assert!(teacher.input(Input::Tick { now_ms: 10_000 }).is_empty());
    // The hook echo cannot advance the step; only the driver's result can.
    assert!(
        teacher
            .input(Input::Click {
                at: rect().center(),
                left: true,
                injected: true,
                now_ms: 10_001,
            })
            .is_empty()
    );
    let mut human = do_it_teacher(vec![click_action(), click_action(), Action::Wait]);
    // The same completion as a person's click, without the praise: Mate pressed it.
    let mut expected = human.input(click(110, 10_002));
    expected.retain(|command| *command != Command::Say(LineKey::RightClick));
    assert_eq!(
        teacher.input(Input::DoItDone {
            ok: true,
            now_ms: 10_002
        }),
        expected
    );
    assert!(
        teacher
            .input(Input::DoItDone {
                ok: true,
                now_ms: 10_003
            })
            .is_empty()
    );
    assert!(
        teacher
            .input(Input::DoItAnswer {
                yes: true,
                now_ms: 10_004
            })
            .is_empty()
    );
    teacher.input(Input::DoItRequested { now_ms: 10_005 });
    teacher.input(Input::DoItAnswer {
        yes: true,
        now_ms: 10_006,
    });
    let out = teacher.input(Input::DoItDone {
        ok: true,
        now_ms: 10_007,
    });
    let mut expected = human.input(click(110, 10_007));
    expected.retain(|command| *command != Command::Say(LineKey::RightClick));
    assert_eq!(out, expected);
    assert!(!ack(&out));
    assert!(teacher.input(Input::Tick { now_ms: 10_606 }).is_empty());
    assert_eq!(
        teacher.input(Input::Tick { now_ms: 10_607 }),
        vec![Command::RequestScreenshot]
    );
}

#[test]
fn declined_unavailable_and_failed_presses_keep_the_step_without_acknowledgement() {
    for outcome in 0..4 {
        let mut teacher = do_it_teacher(vec![click_action()]);
        teacher.input(Input::DoItRequested { now_ms: 1 });
        let (event, line) = match outcome {
            0 => (
                Input::DoItAnswer {
                    yes: false,
                    now_ms: 2,
                },
                LineKey::PressCancelled,
            ),
            1 => (Input::DoItUnavailable { now_ms: 2 }, LineKey::WontPress),
            2 => {
                teacher.input(Input::DoItAnswer {
                    yes: true,
                    now_ms: 2,
                });
                (Input::DoItUnavailable { now_ms: 3 }, LineKey::WontPress)
            }
            _ => {
                teacher.input(Input::DoItAnswer {
                    yes: true,
                    now_ms: 2,
                });
                (
                    Input::DoItDone {
                        ok: false,
                        now_ms: 3,
                    },
                    LineKey::PressFailed,
                )
            }
        };
        assert_eq!(teacher.input(event), vec![Command::Say(line)]);
        assert!(
            teacher
                .input(Input::DoItAnswer {
                    yes: true,
                    now_ms: 4
                })
                .is_empty()
        );
        assert!(
            teacher
                .input(Input::DoItDone {
                    ok: true,
                    now_ms: 5
                })
                .is_empty()
        );
        assert!(teacher.input(Input::Tick { now_ms: 1000 }).is_empty());
        assert!(!ack(&teacher.input(click(110, 1001))));
    }
}

#[test]
fn human_click_clears_offer_and_screen_or_model_change_discards_authorization() {
    let mut teacher = do_it_teacher(vec![click_action(), click_action()]);
    teacher.input(Input::DoItRequested { now_ms: 1 });
    assert!(
        teacher
            .input(click(110, 2))
            .contains(&Command::Say(LineKey::RightClick))
    );
    assert!(
        teacher
            .input(Input::DoItAnswer {
                yes: true,
                now_ms: 3
            })
            .is_empty()
    );
    for screen_change in [false, true] {
        teacher.input(Input::DoItRequested { now_ms: 4 });
        teacher.input(Input::DoItAnswer {
            yes: true,
            now_ms: 5,
        });
        teacher.input(if screen_change {
            Input::ScreenChangedUnexpectedly { now_ms: 6 }
        } else {
            turn(vec![click_action()])
        });
        assert!(
            teacher
                .input(Input::DoItDone {
                    ok: true,
                    now_ms: 7
                })
                .is_empty()
        );
    }
}

#[test]
fn offers_are_opt_in_left_click_only_and_automatic_offer_is_once_per_step() {
    assert!(!TeacherConfig::default().do_it_for_me);
    assert_eq!(TeacherConfig::default().offer_after_failures, 2);
    let mut disabled = Teacher::new(TeacherConfig::default());
    disabled.input(do_it_turn(vec![click_action()]));
    assert!(
        disabled
            .input(Input::DoItRequested { now_ms: 1 })
            .is_empty()
    );
    for now_ms in 2..5 {
        assert!(!disabled.input(click(200, now_ms)).contains(&offer()));
    }
    for action in [
        Action::DoubleClick { x: 150, y: 100 },
        drag_action(),
        Action::Type {
            text: "hello".into(),
        },
        Action::Keypress {
            keys: vec!["Enter".into()],
        },
    ] {
        let mut teacher = do_it_teacher(vec![action]);
        assert_eq!(
            teacher.input(Input::DoItRequested { now_ms: 1 }),
            vec![Command::Say(LineKey::PressFailed)]
        );
        assert!(!teacher.input(click(200, 2)).contains(&offer()));
        assert!(!teacher.input(click(200, 3)).contains(&offer()));
    }
    let mut teacher = do_it_teacher(vec![click_action(), click_action()]);
    assert!(!teacher.input(click(200, 1)).contains(&offer()));
    assert!(teacher.input(click(200, 2)).contains(&offer()));
    teacher.input(Input::DoItAnswer {
        yes: false,
        now_ms: 3,
    });
    for now_ms in 4..8 {
        assert!(!teacher.input(click(200, now_ms)).contains(&offer()));
    }
    // A fresh explicit request may offer again after a refusal.
    assert_eq!(
        teacher.input(Input::DoItRequested { now_ms: 8 }),
        vec![offer()]
    );
    teacher.input(click(110, 9));
    assert!(!teacher.input(click(200, 10)).contains(&offer()));
    assert!(teacher.input(click(200, 11)).contains(&offer()));
    let mut custom = Teacher::new(TeacherConfig {
        do_it_for_me: true,
        offer_after_failures: 3,
        ..Default::default()
    });
    custom.input(do_it_turn(vec![click_action()]));
    assert!(!custom.input(click(200, 1)).contains(&offer()));
    assert!(!custom.input(click(200, 2)).contains(&offer()));
    assert!(custom.input(click(200, 3)).contains(&offer()));
}
