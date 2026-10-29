//! Pure human-operated teacher loop. Call `Teacher::input` for each event.
//!
//! Model turns without timestamps use the latest input time (initially zero).
//! Scroll, drag and non-left clicks fail closed: the input protocol cannot
//! verify those gestures. Targets are already snapped in physical coordinates;
//! the frame is never used to manufacture a fallback target.

mod clicks;
mod command;
mod config;
mod confirmation;
mod input;
mod keys;
mod line;
mod model;
mod offer;
mod refusal;
mod state;
mod step;

pub use clicks::ClickRecognizer;
pub use command::TeacherCommand;
pub use config::TeacherConfig;
pub use input::TeacherInput;
pub use keys::{ChordRecognizer, KeyWindow, canonical_key_name, vk_to_key_name};
pub use line::LineKey;
pub use model::ModelChoice;
pub use offer::{confirmation_answer, confirmation_ready_at};
pub use refusal::RefusalReason;
pub use state::Teacher;
