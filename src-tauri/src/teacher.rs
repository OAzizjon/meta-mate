//! "How do I ...?" (docs/architecture.md section 3 B, docs/plan.md Day 5): computer use
//! with the person as the hand. Muse Spark sees the redacted window and answers with a
//! `computer_call`. `mm_core::teacher::Teacher` turns each turn into
//! steps; Mate points at each step (pointing.rs) and waits for the person's own click
//! (the mouse hook: injected clicks never count, invariant 4). After the last step a new
//! look goes back to the model as the `computer_call_output`, with the safety checks the
//! person's clicks confirmed, until the model says it is done (or the turn budget ends).
//! An optional left click requires a separate spoken yes and verified injection.
//!
//! The lesson is one generation of the answer stream: commentary lines are sentences of
//! its timeline, cached lines (wrong place, done) are spoken as sentence 0. A lesson the
//! model finished leaves a keepsake card of its commentary (card.rs).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use mm_core::api::intent::asks_do_it;
use mm_core::api::request::{
    ComputerSession, ModelChoice, ReasoningEffort, computer_followup, computer_request,
};
use mm_core::api::response::{Action, ParsedResponse};
use mm_core::geom::{PhysRect, ShotPoint};
use mm_core::matescript::{Bound, Item, Say};
use mm_core::redaction::{overlaps_any, point_in_any};
use mm_core::teacher::{
    ChordRecognizer, ClickRecognizer, LineKey, RefusalReason, Teacher, TeacherCommand,
    TeacherConfig, TeacherInput, confirmation_answer, confirmation_ready_at,
};
use mm_platform::{InputAction, InputInjector, MouseButton};
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, oneshot};

use crate::answer_stream::{AnswerMsg, AnswerStream, model_chip};
use crate::pointing::{Pointing, show_target, stop_pointing};
use crate::proxy::{CallKind, Proxy};
use crate::screen::{Look, LookFailure};
use crate::voice::{Phrases, say_line, voice_of};

/// How often the teacher's clock ticks (settle time, timeouts).
const TICK: Duration = Duration::from_millis(100);
/// The longest a lesson waits for progress (a step done or a new turn); stray clicks do
/// not extend it.
const IDLE_LIMIT: Duration = Duration::from_secs(180);
/// The longest a lesson may run at all: the mouse hook is system-wide.
const LESSON_LIMIT: Duration = Duration::from_secs(600);
const OFFER_LIMIT: Duration = Duration::from_secs(30);
/// The pause before a failed model turn is asked again.
const RETRY_PAUSE: Duration = Duration::from_millis(700);

/// Recognized speech and a reply indicating whether the lesson consumed it.
pub(crate) struct Words {
    pub pressed_at: Option<Instant>,
    pub text: String,
    pub taken: oneshot::Sender<bool>,
}

mod inputs;
mod lesson;
mod presentation;

use inputs::{listen_for_clicks, listen_for_keys};
pub(crate) use lesson::teach;
use presentation::*;

#[cfg(test)]
use lesson::{fresh_answer, input_staleness, screenshot_wait_expired};

#[cfg(test)]
mod tests;
