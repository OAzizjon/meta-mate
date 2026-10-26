use super::ModelChoice;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeacherConfig {
    pub configured_model: ModelChoice,
    pub auto_escalate: bool,
    pub max_turns: u8,
    pub hit_tolerance_px: u32,
    pub near_px: u32,
    pub settle_ms: u64,
}

impl Default for TeacherConfig {
    fn default() -> Self {
        Self {
            configured_model: ModelChoice::Quick,
            auto_escalate: false,
            max_turns: 12,
            hit_tolerance_px: 8,
            near_px: 40,
            settle_ms: 600,
        }
    }
}
