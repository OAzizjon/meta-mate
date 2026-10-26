use crate::geom::PhysRect;

use super::{LineKey, ModelChoice, RefusalReason};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeacherCommand {
    PointAt { rect: PhysRect, step: u8, total: u8 },
    Say(LineKey),
    SpeakCommentary(String),
    RequestScreenshot,
    AcknowledgeSafetyChecks(Vec<String>),
    UseModel(ModelChoice),
    Finished,
    Refused(RefusalReason),
}
