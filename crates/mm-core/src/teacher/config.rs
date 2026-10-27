use super::ModelChoice;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeacherConfig {
    pub configured_model: ModelChoice,
    pub auto_escalate: bool,
    pub keyboard_steps: bool,
    pub do_it_for_me: bool,
    pub offer_after_failures: u8,
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
            keyboard_steps: false,
            do_it_for_me: false,
            offer_after_failures: 2,
            max_turns: 12,
            hit_tolerance_px: 8,
            near_px: 40,
            settle_ms: 600,
        }
    }
}
