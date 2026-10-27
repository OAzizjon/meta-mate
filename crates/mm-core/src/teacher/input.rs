use std::fmt;

use crate::api::response::ParsedResponse;
use crate::geom::{FrameTransform, PhysPoint, PhysRect};

/// `left`: the left button (only a left click completes a click step; a right click opens
/// a menu instead). `injected`: the hook's injected flag (`LLMHF_INJECTED` /
/// `LLKHF_INJECTED`): input made by software, ours included, never completes a step or
/// confirms a safety check (invariant 4).
#[derive(Clone)]
pub enum TeacherInput {
    DoItRequested {
        now_ms: u64,
    },
    DoItAnswer {
        yes: bool,
        now_ms: u64,
    },
    DoItUnavailable {
        now_ms: u64,
    },
    DoItDone {
        ok: bool,
        now_ms: u64,
    },
    ModelTurn {
        response: ParsedResponse,
        frame: FrameTransform,
        targets: Vec<Option<PhysRect>>,
    },
    Click {
        at: PhysPoint,
        left: bool,
        injected: bool,
        now_ms: u64,
    },
    /// The adapter has already recognized the system double-click gesture.
    DoubleClick {
        at: PhysPoint,
        left: bool,
        injected: bool,
        now_ms: u64,
    },
    KeyChord {
        keys: Vec<String>,
        injected: bool,
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

/// Metadata only (convention 3): a field's value can be a revealed password (D37) and
/// key chords can be typing.
impl fmt::Debug for TeacherInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DoItRequested { now_ms } => write!(f, "DoItRequested({now_ms})"),
            Self::DoItAnswer { yes, now_ms } => f
                .debug_struct("DoItAnswer")
                .field("yes", yes)
                .field("now_ms", now_ms)
                .finish(),
            Self::DoItUnavailable { now_ms } => write!(f, "DoItUnavailable({now_ms})"),
            Self::DoItDone { ok, now_ms } => f
                .debug_struct("DoItDone")
                .field("ok", ok)
                .field("now_ms", now_ms)
                .finish(),
            Self::ModelTurn {
                response,
                frame,
                targets,
            } => f
                .debug_struct("ModelTurn")
                .field("response", response)
                .field("frame", frame)
                .field("targets", targets)
                .finish(),
            Self::Click {
                at,
                left,
                injected,
                now_ms,
            } => f
                .debug_struct("Click")
                .field("at", at)
                .field("left", left)
                .field("injected", injected)
                .field("now_ms", now_ms)
                .finish(),
            Self::DoubleClick {
                at,
                left,
                injected,
                now_ms,
            } => f
                .debug_struct("DoubleClick")
                .field("at", at)
                .field("left", left)
                .field("injected", injected)
                .field("now_ms", now_ms)
                .finish(),
            Self::KeyChord {
                keys,
                injected,
                now_ms,
            } => f
                .debug_struct("KeyChord")
                .field("keys", &keys.len())
                .field("injected", injected)
                .field("now_ms", now_ms)
                .finish(),
            Self::ValueChanged { text, now_ms } => f
                .debug_struct("ValueChanged")
                .field("chars", &text.chars().count())
                .field("now_ms", now_ms)
                .finish(),
            Self::UserSaidNext { now_ms } => write!(f, "UserSaidNext({now_ms})"),
            Self::ScreenChangedUnexpectedly { now_ms } => {
                write!(f, "ScreenChangedUnexpectedly({now_ms})")
            }
            Self::Tick { now_ms } => write!(f, "Tick({now_ms})"),
            Self::ModelFailed { now_ms } => write!(f, "ModelFailed({now_ms})"),
            Self::WonderUnavailable => f.write_str("WonderUnavailable"),
        }
    }
}
