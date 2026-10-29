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

fn drag_action() -> Action {
    // The first point maps to (100, 100); the end is outside the target.
    Action::Drag {
        path: vec![(150, 100), (200, 100)],
    }
}

mod teacher {
    use super::*;

    mod do_it;
    mod gestures;
    mod lifecycle;
}
