use super::*;

#[test]
fn screenshot_wait_has_its_own_deadline_and_keeps_lesson_limits() {
    assert!(!screenshot_wait_expired(
        Duration::ZERO,
        Duration::ZERO,
        Duration::ZERO,
    ));
    assert!(screenshot_wait_expired(
        OFFER_LIMIT,
        Duration::ZERO,
        Duration::ZERO,
    ));
    assert!(screenshot_wait_expired(
        Duration::ZERO,
        IDLE_LIMIT,
        Duration::ZERO,
    ));
    assert!(screenshot_wait_expired(
        Duration::ZERO,
        Duration::ZERO,
        LESSON_LIMIT,
    ));
}

#[test]
fn only_a_ptt_press_strictly_after_the_offer_can_answer() {
    let offered = Instant::now();
    assert!(!fresh_answer(None, Some(offered)));
    assert!(!fresh_answer(Some(offered), None));
    assert!(!fresh_answer(Some(offered), Some(offered)));
    assert!(!fresh_answer(
        Some(offered - Duration::from_millis(1)),
        Some(offered)
    ));
    assert!(fresh_answer(
        Some(offered + Duration::from_millis(1)),
        Some(offered)
    ));
}

#[test]
fn chord_staleness_uses_the_event_window_and_step_time() {
    let target = mm_platform::TargetWindow {
        handle: 12,
        process_id: 34,
    };
    let chord = |handle, process_id, now_ms| TeacherInput::KeyChord {
        foreground: Some(mm_core::teacher::KeyWindow { handle, process_id }),
        keys: vec!["esc".into()],
        injected: false,
        now_ms,
    };
    // No live foreground query: closing/opening a dialog cannot invalidate this key.
    assert_eq!(
        input_staleness(&chord(12, 34, 20), 20, target),
        (false, false)
    );
    // A separate same-process window cannot complete this window's step.
    assert_eq!(
        input_staleness(&chord(13, 34, 20), 20, target),
        (true, true)
    );
    assert_eq!(
        input_staleness(&chord(12, 35, 20), 20, target),
        (true, true)
    );
    assert_eq!(
        input_staleness(&chord(12, 34, 19), 20, target),
        (true, false)
    );
    assert_eq!(
        input_staleness(&chord(13, 35, 19), 20, target),
        (true, false)
    );
}

#[test]
fn only_wonder_served_by_spark_is_a_fallback() {
    assert!(fell_back(ModelChoice::Thorough, Some("muse-spark-1.3")));
    assert!(fell_back(ModelChoice::Thorough, Some("muse-spark")));
    assert!(!fell_back(ModelChoice::Quick, Some("muse-spark-1.3")));
    assert!(!fell_back(ModelChoice::Thorough, Some("muse-wonder-1.0")));
    assert!(!fell_back(ModelChoice::Thorough, Some("other-model")));
    assert!(!fell_back(ModelChoice::Thorough, Some("")));
    assert!(!fell_back(ModelChoice::Thorough, None));
}

fn offer_snapshot() -> mm_platform::UiSnapshot {
    let frame = PhysRect::new(0, 0, 100, 100).unwrap();
    let element =
        |id: &str, parent: Option<&str>, kind: &str, name: &str, rect| mm_platform::UiElement {
            id: id.into(),
            parent: parent.map(str::to_owned),
            control_type: kind.into(),
            name: name.into(),
            rect: Some(rect),
            interactive: kind == "Button",
            enabled: true,
            offscreen: false,
            is_password: false,
            password_like: false,
        };
    let mut elements = vec![
        element("window", None, "Window", "Settings", frame),
        element("pane", Some("window"), "Document", "Options", frame),
        element(
            "button",
            Some("pane"),
            "Button",
            "Open",
            PhysRect::new(10, 10, 30, 30).unwrap(),
        ),
    ];
    // A real options page has readable content around its button. Keep these lines
    // clear of the button and of the box the drag test paints.
    for (i, line) in [
        "Open files in this app",
        "Show recent files",
        "Start where you left off",
        "Theme: use system setting",
        "Language: English",
        "Check for updates automatically",
        "Version 2.3.1",
    ]
    .into_iter()
    .enumerate()
    {
        let top = 10 + 5 * i32::try_from(i).unwrap();
        elements.push(element(
            &format!("text{i}"),
            Some("pane"),
            "Text",
            line,
            PhysRect::new(40, top, 95, top + 4).unwrap(),
        ));
    }
    mm_platform::UiSnapshot {
        target: mm_platform::TargetWindow {
            handle: 1,
            process_id: 1,
        },
        frame,
        truncated: false,
        elements,
    }
}

fn prepared_look(painted: &[PhysRect]) -> Look {
    let capture = mm_platform::CapturedImage::new(100, 100, vec![200; 100 * 100 * 4]).unwrap();
    crate::screen::prepare(offer_snapshot(), &capture, painted).unwrap()
}

#[test]
fn selected_model_survives_retries_and_followups() {
    let look = prepared_look(&[]);
    for selected in [
        crate::settings::Model::Quick,
        crate::settings::Model::Thorough,
    ] {
        let model = ModelChoice::from(selected);
        let mut session = lesson_session(model, look.redacted.frame.sent_size());
        let expected = serde_json::to_value(model).unwrap();
        for effort in [ReasoningEffort::Minimal, ReasoningEffort::Low] {
            session.effort = effort;
            let first = computer_request(
                session.model,
                session.effort,
                "Open options",
                &look.list,
                &look.redacted.image,
            );
            let next = computer_followup(session, "response", "call", &[], &look.redacted.image);
            assert_eq!(first["model"], expected);
            assert_eq!(next["model"], expected);
        }
    }
}

#[test]
fn drags_refuse_painted_starts_and_ends() {
    let response = |path| ParsedResponse {
        id: "response".into(),
        model: String::new(),
        status: "completed".into(),
        commentary: Vec::new(),
        computer_calls: vec![mm_core::api::response::ComputerCall {
            call_id: "call".into(),
            actions: vec![Action::Drag { path }],
            safety_checks: Vec::new(),
            replay_receipt: None,
        }],
        usage: None,
        incomplete_reason: None,
        reasoning_items: 0,
    };
    let painted = PhysRect::new(60, 60, 80, 80).unwrap();
    let look = prepared_look(&[painted]);
    let target = look.snapshot.elements[2].rect;
    assert_eq!(
        targets_for(&look, &response(vec![(20, 20), (50, 50)])),
        vec![target]
    );
    for path in [
        vec![(20, 20), (60, 60)],
        vec![(60, 60), (20, 20)],
        vec![(20, 20), (100, 50)],
        Vec::new(),
    ] {
        assert_eq!(targets_for(&look, &response(path)), vec![None]);
    }
}

#[test]
fn popup_roots_still_check_the_target_window_title() {
    for kind in ["Menu", "List", "Pane"] {
        let mut snapshot = offer_snapshot();
        let rect = snapshot.elements[2].rect.unwrap();
        snapshot.elements[1].parent = None;
        snapshot.elements[1].control_type = kind.into();
        snapshot.elements[2].control_type = "MenuItem".into();
        assert!(pressable(&snapshot, rect), "{kind}");
        snapshot.elements[0].name = "Delete file?".into();
        assert!(!pressable(&snapshot, rect));
        snapshot.elements[0].name = "Settings".into();
        snapshot.elements[1].name = "Payment".into();
        assert!(!pressable(&snapshot, rect));
        snapshot.elements[1].name.clear();
        snapshot.elements[2].name.clear();
        assert!(!pressable(&snapshot, rect));
        snapshot.elements[2].name = "Open".into();
        snapshot.elements[1].is_password = true;
        assert!(!pressable(&snapshot, rect));
    }
}

#[test]
fn popup_roots_need_a_target_window_and_complete_snapshot() {
    let mut snapshot = offer_snapshot();
    let rect = snapshot.elements[2].rect.unwrap();
    snapshot.elements[1].parent = None;
    snapshot.elements[1].control_type = "Menu".into();
    snapshot.truncated = true;
    assert!(!pressable(&snapshot, rect));
    snapshot.truncated = false;
    snapshot.elements[0].control_type = "Pane".into();
    assert!(!pressable(&snapshot, rect));
    snapshot.elements[0].control_type = "Window".into();
    snapshot.elements[1].control_type = "Custom".into();
    assert!(!pressable(&snapshot, rect));
}

#[test]
fn offers_check_forbidden_and_hidden_ancestors() {
    let mut snapshot = offer_snapshot();
    let rect = snapshot.elements[2].rect.unwrap();
    assert!(pressable(&snapshot, rect));
    snapshot.elements[0].name = "Delete file?".into();
    assert!(!pressable(&snapshot, rect));
    snapshot.elements[0].name = "Settings".into();
    snapshot.elements[1].is_password = true;
    assert!(!pressable(&snapshot, rect));
}

#[test]
fn container_names_do_not_identify_an_unnamed_button() {
    let mut snapshot = offer_snapshot();
    let rect = snapshot.elements[2].rect.unwrap();
    snapshot.elements[2].name.clear();
    assert!(!pressable(&snapshot, rect));
    snapshot.elements[1].control_type = "Button".into();
    assert!(!pressable(&snapshot, rect));
    snapshot.elements[2].control_type = "Image".into();
    assert!(pressable(&snapshot, rect));
    for parent in ["ListItem", "ToolBar", "Custom"] {
        snapshot.elements[1].control_type = parent.into();
        assert!(!pressable(&snapshot, rect));
    }
}

#[test]
fn incomplete_and_cyclic_ancestry_fail_closed() {
    let mut snapshot = offer_snapshot();
    let rect = snapshot.elements[2].rect.unwrap();
    snapshot.elements[1].parent = Some("missing".into());
    assert!(!pressable(&snapshot, rect));
    snapshot.elements[1].parent = Some("button".into());
    assert!(!pressable(&snapshot, rect));
}

#[test]
fn every_line_the_teacher_says_has_a_locale_key_or_its_commentary() {
    for line in [
        LineKey::WrongPlace,
        LineKey::WrongKeys,
        LineKey::DoubleClick,
        LineKey::ScreenChanged,
        LineKey::RightClick,
        LineKey::CantFind,
        LineKey::Done,
        LineKey::NetworkError,
        LineKey::TooManyTurns,
        LineKey::WontPress,
        LineKey::DoItForMe,
        LineKey::Pressing,
        LineKey::PressCancelled,
        LineKey::PressFailed,
    ] {
        let key = line_key(line, &mut 0).unwrap();
        let en: serde_json::Value =
            serde_json::from_str(include_str!("../../../locales/en.json")).unwrap();
        assert!(en.get(key).is_some(), "{key} missing");
    }
    assert_eq!(line_key(LineKey::StepN, &mut 0), None);
}

#[test]
fn praise_rotates_through_every_line_in_both_languages() {
    let en: serde_json::Value =
        serde_json::from_str(include_str!("../../../locales/en.json")).unwrap();
    let ru: serde_json::Value =
        serde_json::from_str(include_str!("../../../locales/ru.json")).unwrap();
    let mut done = 0;
    let said: Vec<_> = (0..5)
        .map(|_| line_key(LineKey::RightClick, &mut done).unwrap())
        .collect();
    assert_eq!(said[0], "robot.right_click");
    assert_eq!(said[4], "robot.right_click");
    for key in &said[..4] {
        assert!(
            en.get(key).is_some() && ru.get(key).is_some(),
            "{key} missing"
        );
    }
    assert_eq!(done, 5);
}
