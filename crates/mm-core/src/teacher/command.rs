use std::fmt;

use crate::geom::{PhysPoint, PhysRect};

use super::{LineKey, ModelChoice, RefusalReason};

#[derive(Clone, PartialEq, Eq)]
pub enum TeacherCommand {
    PointAt { rect: PhysRect, step: u8, total: u8 },
    OfferDoIt { rect: PhysRect },
    PerformClick { at: PhysPoint, rect: PhysRect },
    Say(LineKey),
    SpeakCommentary(String),
    RequestScreenshot,
    AcknowledgeSafetyChecks(Vec<String>),
    UseModel(ModelChoice),
    Finished,
    Refused(RefusalReason),
}

/// Metadata only for the model's spoken line (convention 3), like `ParsedResponse`.
impl fmt::Debug for TeacherCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OfferDoIt { rect } => f.debug_struct("OfferDoIt").field("rect", rect).finish(),
            Self::PerformClick { at, rect } => f
                .debug_struct("PerformClick")
                .field("at", at)
                .field("rect", rect)
                .finish(),
            Self::PointAt { rect, step, total } => f
                .debug_struct("PointAt")
                .field("rect", rect)
                .field("step", step)
                .field("total", total)
                .finish(),
            Self::Say(line) => f.debug_tuple("Say").field(line).finish(),
            Self::SpeakCommentary(text) => f
                .debug_struct("SpeakCommentary")
                .field("chars", &text.chars().count())
                .finish(),
            Self::RequestScreenshot => f.write_str("RequestScreenshot"),
            Self::AcknowledgeSafetyChecks(ids) => {
                f.debug_tuple("AcknowledgeSafetyChecks").field(ids).finish()
            }
            Self::UseModel(model) => f.debug_tuple("UseModel").field(model).finish(),
            Self::Finished => f.write_str("Finished"),
            Self::Refused(reason) => f.debug_tuple("Refused").field(reason).finish(),
        }
    }
}
