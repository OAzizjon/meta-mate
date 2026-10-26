use crate::api::response::ParsedResponse;
use crate::geom::{FrameTransform, PhysPoint, PhysRect};

#[derive(Debug, Clone)]
pub enum TeacherInput {
    ModelTurn {
        response: ParsedResponse,
        frame: FrameTransform,
        targets: Vec<Option<PhysRect>>,
    },
    Click {
        at: PhysPoint,
        now_ms: u64,
    },
    /// The adapter has already recognized the system double-click gesture.
    DoubleClick {
        at: PhysPoint,
        now_ms: u64,
    },
    KeyChord {
        keys: Vec<String>,
        now_ms: u64,
    },
    ValueChanged {
        text: String,
        now_ms: u64,
    },
    UserSaidNext {
        now_ms: u64,
    },
    ScreenChangedUnexpectedly {
        now_ms: u64,
    },
    Tick {
        now_ms: u64,
    },
    ModelFailed {
        now_ms: u64,
    },
    WonderUnavailable,
}
