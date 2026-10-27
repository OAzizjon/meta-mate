use mm_core::api::response::{
    Action, ResponseError, StreamText, parse_response, parse_response_value,
};
use serde_json::{Value, json};

macro_rules! replay {
    ($path:literal) => {
        include_str!(concat!("../../../fixtures/replay/", $path))
    };
}

const META: &[&str] = &[
    replay!("stream/probe-20261026T183304567651Z-meta.json"),
    replay!("stream/probe-20261026T183322159011Z-meta.json"),
    replay!("stream/probe-20261026T183330559068Z-meta.json"),
    replay!("stream/probe-20261026T183403381738Z-meta.json"),
    replay!("stream/probe-20261026T183407175757Z-meta.json"),
    replay!("synthetic-text-size/probe-20261026T183422420739Z-meta.json"),
    replay!("synthetic-text-size/probe-20261026T183429804717Z-meta.json"),
];

fn recording(body: &str) -> Value {
    serde_json::from_str::<Value>(body).unwrap()["response"].clone()
}

fn body(actions: Value) -> Value {
    json!({
        "id": "resp_test", "model": "muse-spark-1.3", "status": "completed",
        "output": [{"type": "computer_call", "call_id": "call_test", "actions": actions}]
    })
}

#[test]
fn all_meta_recordings_preserve_text_usage_and_reasoning_counts() {
    for fixture in META {
        let raw = recording(fixture);
        let parsed = parse_response_value(&raw).unwrap();
        assert_eq!(parse_response(&raw.to_string()).unwrap(), parsed);
        assert_eq!(parsed.id, raw["id"]);
        assert_eq!(parsed.model, raw["model"]);
        assert_eq!(parsed.status, "completed");
        assert_eq!(parsed.reasoning_items, 1);
        assert_eq!(
            parsed.commentary,
            [raw["output"][1]["content"][0]["text"].as_str().unwrap()]
        );
        let usage = parsed.usage.unwrap();
        assert_eq!(
            usage.input_tokens,
            raw["usage"]["input_tokens"].as_u64().unwrap()
        );
        assert_eq!(
            usage.output_tokens,
            raw["usage"]["output_tokens"].as_u64().unwrap()
        );
        assert_eq!(
            usage.reasoning_tokens,
            raw["usage"]["output_tokens_details"]["reasoning_tokens"].as_u64()
        );
        assert_eq!(parsed.incomplete_reason, None);
    }
}

#[test]
fn recorded_clicks_keep_coordinates_safety_and_receipts() {
    for (fixture, y) in [(META[5], 270), (META[6], 271)] {
        let raw = recording(fixture);
        let parsed = parse_response_value(&raw).unwrap();
        assert_eq!(parsed.commentary.len(), 1);
        assert_eq!(parsed.computer_calls.len(), 1);
        let call = &parsed.computer_calls[0];
        assert_eq!(
            call.actions,
            [Action::Click {
                x: 1059,
                y,
                button: "left".into()
            }]
        );
        assert_eq!(call.call_id, raw["output"][2]["call_id"]);
        assert_eq!(call.safety_checks.len(), 1);
        assert_eq!(
            call.safety_checks[0].code,
            "computer_action_requires_confirmation"
        );
        assert_eq!(
            call.safety_checks[0].id,
            raw["output"][2]["pending_safety_checks"][0]["id"]
        );
        assert_eq!(
            call.safety_checks[0].message,
            raw["output"][2]["pending_safety_checks"][0]["message"]
        );
        assert_eq!(
            call.replay_receipt.as_deref(),
            raw["output"][2]["meta_safety_replay_receipt"].as_str()
        );
    }
}

#[test]
fn foreign_recordings_are_only_shape_compatibility_inputs() {
    for fixture in [
        replay!("stream/probe-20261026T050749Z-openrouter.json"),
        replay!("synthetic-text-size/probe-20261026T050802Z-openrouter.json"),
        replay!("synthetic-text-size/probe-20261026T050806Z-openrouter.json"),
        replay!("synthetic-text-size/probe-20261026T051427Z-openrouter.json"),
        replay!("synthetic-text-size/probe-20261026T051743137891Z-openrouter.json"),
    ] {
        assert!(parse_response_value(&recording(fixture)).is_ok());
    }
}

#[test]
fn singular_action_and_all_action_variants() {
    let mut raw = body(json!([]));
    raw["output"][0].as_object_mut().unwrap().remove("actions");
    raw["output"][0]["action"] = json!({"type": "click", "x": -10, "y": 20, "button": "right"});
    assert_eq!(
        parse_response_value(&raw).unwrap().computer_calls[0].actions,
        [Action::Click {
            x: -10,
            y: 20,
            button: "right".into()
        }]
    );

    let raw = body(json!([
        {"type": "double_click", "x": 1, "y": 2},
        {"type": "scroll", "x": 3, "y": 4, "scroll_x": -5, "scroll_y": 6},
        {"type": "type", "text": "private text"},
        {"type": "keypress", "keys": ["CTRL", "A"]},
        {"type": "drag", "path": [{"x": 7, "y": 8}, {"x": 9, "y": 10}]},
        {"type": "move", "x": 11, "y": 12},
        {"type": "wait"}, {"type": "screenshot"},
        {"type": "future_action", "payload": {"ignored": true}}
    ]));
    assert_eq!(
        parse_response_value(&raw).unwrap().computer_calls[0].actions,
        [
            Action::DoubleClick { x: 1, y: 2 },
            Action::Scroll {
                x: 3,
                y: 4,
                scroll_x: -5,
                scroll_y: 6
            },
            Action::Type {
                text: "private text".into()
            },
            Action::Keypress {
                keys: vec!["CTRL".into(), "A".into()]
            },
            Action::Drag {
                path: vec![(7, 8), (9, 10)]
            },
            Action::Move { x: 11, y: 12 },
            Action::Wait,
            Action::Screenshot,
            Action::Unknown {
                kind: "future_action".into()
            },
        ]
    );
}

#[test]
fn filters_phases_and_ignores_reasoning_content_and_extra_fields() {
    let raw = json!({
        "id": "r", "model": "m", "status": "completed", "future": true,
        "output": [
            {"type": "reasoning", "content": "never exposed"},
            {"type": "message", "content": [{"type": "output_text", "text": "one"}]},
            {"type": "message", "phase": "commentary", "content": [{"type": "text", "text": "two"}]},
            {"type": "message", "phase": "final_answer", "content": [{"type": "output_text", "text": "skip"}]},
            {"type": "future_output"}
        ],
        "usage": {"input_tokens": 1, "output_tokens": 2}
    });
    let parsed = parse_response_value(&raw).unwrap();
    assert_eq!(parsed.commentary, ["one", "two"]);
    assert_eq!(parsed.reasoning_items, 1);
    assert_eq!(parsed.usage.unwrap().reasoning_tokens, None);
}

#[test]
fn malformed_inputs_are_errors_and_api_messages_are_bounded_unicode() {
    assert!(matches!(
        parse_response("{"),
        Err(ResponseError::InvalidJson { .. })
    ));
    for raw in [
        json!(null),
        json!([]),
        json!({}),
        body(json!([{"type": "click", "x": 1.5, "y": 2, "button": "left"}])),
    ] {
        assert!(matches!(
            parse_response_value(&raw),
            Err(ResponseError::InvalidShape { .. })
        ));
    }
    let message = "ж".repeat(250);
    let error =
        parse_response_value(&json!({"error": {"message": format!("  {message}  ")}})).unwrap_err();
    assert_eq!(
        error,
        ResponseError::Api {
            message: "ж".repeat(200)
        }
    );
    assert!(
        matches!(parse_response(r#"{"error":"unavailable"}"#), Err(ResponseError::Api { message }) if message == "unavailable")
    );
}

#[test]
fn stream_accumulates_deltas_and_returns_completed_and_incomplete_responses() {
    let mut stream = StreamText::new();
    stream
        .push(Some("response.output_text.delta"), r#"{"delta":"Hello "}"#)
        .unwrap();
    stream
        .push(
            None,
            r#"{"type":"response.output_text.delta","delta":"world"}"#,
        )
        .unwrap();
    stream.push(Some("future.event"), "{}").unwrap();
    stream.push(None, "[DONE]").unwrap();
    assert_eq!(stream.text(), "Hello world");
    let raw = recording(META[0]);
    let event = json!({"response": raw}).to_string();
    assert_eq!(
        stream.push(Some("response.completed"), &event).unwrap(),
        Some(parse_response_value(&raw).unwrap())
    );
    assert_eq!(stream.text(), "Hello world");

    // All checked-in Meta streams completed; construct the documented token-limit case.
    let mut incomplete = raw;
    incomplete["status"] = json!("incomplete");
    incomplete["incomplete_details"] = json!({"reason": "max_output_tokens"});
    let event = json!({"type": "response.incomplete", "response": incomplete}).to_string();
    let parsed = stream.push(None, &event).unwrap().unwrap();
    assert_eq!(parsed.status, "incomplete");
    assert_eq!(
        parsed.incomplete_reason.as_deref(),
        Some("max_output_tokens")
    );
    assert!(stream.push(None, "{").is_err());
    assert!(
        stream
            .push(Some("response.output_text.delta"), "{}")
            .is_err()
    );
    assert!(stream.push(Some("response.completed"), "{}").is_err());
    assert!(matches!(
        stream.push(Some("error"), r#"{"error":{"message":"failed"}}"#),
        Err(ResponseError::Api { .. })
    ));
}

#[test]
fn debug_hides_commentary_and_typed_text_through_containers() {
    let mut raw = body(json!([{"type": "type", "text": "typed secret"}]));
    raw["output"].as_array_mut().unwrap().push(json!({
        "type": "message", "content": [{"type": "output_text", "text": "spoken secret"}]
    }));
    let parsed = parse_response_value(&raw).unwrap();
    let debug = format!("{parsed:?}");
    assert!(!debug.contains("typed secret"));
    assert!(!debug.contains("spoken secret"));
    assert!(debug.contains("commentary_chars: [13]"));
    assert!(debug.contains("Type { chars: 12 }"));
    let mut stream = StreamText::new();
    stream
        .push(
            None,
            r#"{"type":"response.output_text.delta","delta":"secret"}"#,
        )
        .unwrap();
    assert_eq!(format!("{stream:?}"), "StreamText { chars: 6 }");
}

#[test]
fn stream_text_is_bounded() {
    let mut stream = StreamText::new();
    let delta = serde_json::json!({"delta": "x".repeat(40 * 1024)}).to_string();
    stream
        .push(Some("response.output_text.delta"), &delta)
        .unwrap();
    assert_eq!(
        stream.push(Some("response.output_text.delta"), &delta),
        Err(ResponseError::TooLong)
    );
    assert_eq!(stream.text().len(), 40 * 1024);
}
