use super::*;

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
        foreground: None,
        keys: vec!["Enter".into()],
        injected: true,
        now_ms: 1,
    });
    assert!(injected.is_empty());
    assert!(ack(&teacher.input(click(110, 2))));
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
