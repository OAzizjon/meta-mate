//! "How do I ...?" (docs/architecture.md section 3 B, docs/plan.md Day 5): computer use
//! with the person as the hand. Muse Spark sees the redacted window and answers with a
//! `computer_call`; we never execute it. `mm_core::teacher::Teacher` turns each turn into
//! steps; Mate points at each step (pointing.rs) and waits for the person's own click
//! (the mouse hook: injected clicks never count, invariant 4). After the last step a new
//! look goes back to the model as the `computer_call_output`, with the safety checks the
//! person's clicks confirmed, until the model says it is done (or the turn budget ends).
//!
//! The lesson is one generation of the answer stream: commentary lines are sentences of
//! its timeline, cached lines (wrong place, done) are spoken as sentence 0.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mm_core::api::request::{
    ComputerSession, ModelChoice, ReasoningEffort, computer_followup, computer_request,
};
use mm_core::api::response::{Action, ParsedResponse};
use mm_core::geom::{PhysRect, ShotPoint};
use mm_core::matescript::{Item, Say};
use mm_core::teacher::{
    ClickRecognizer, LineKey, RefusalReason, Teacher, TeacherCommand, TeacherConfig, TeacherInput,
};
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

use crate::answer_stream::{AnswerMsg, AnswerStream};
use crate::pointing::{Pointing, show_target, stop_pointing};
use crate::proxy::{CallKind, Proxy};
use crate::screen::{Look, LookFailure, take_look};
use crate::voice::{Phrases, say_line, voice_of};

/// How often the teacher's clock ticks (settle time, timeouts).
const TICK: Duration = Duration::from_millis(100);
/// The longest a lesson waits for the person between two of their actions.
const IDLE_LIMIT: Duration = Duration::from_secs(180);

/// The locale key of a teacher line; None for a line that needs words the model gives
/// (the step's own commentary says it).
fn line_key(line: LineKey) -> Option<&'static str> {
    match line {
        LineKey::WrongPlace => Some("robot.wrong_place"),
        LineKey::RightClick => Some("robot.right_click"),
        LineKey::CantFind => Some("robot.cant_find"),
        LineKey::StepN => None,
        LineKey::Done => Some("robot.done"),
        LineKey::NetworkError => Some("robot.network_error"),
        LineKey::TooManyTurns => Some("robot.too_many_turns"),
        LineKey::WontPress => Some("robot.wont_press"),
    }
}

/// The rectangle each action of a turn is about, in the order the teacher reads them: the
/// pointable element under a click, the element under a scroll or move, the whole look
/// for typing, keys and waiting; None for what cannot be placed (the teacher refuses).
fn targets_for(look: &Look, response: &ParsedResponse) -> Vec<Option<PhysRect>> {
    let frame = look.redacted.frame;
    let at = |x: i64, y: i64| {
        let shot = ShotPoint::new(x as f64 + 0.5, y as f64 + 0.5).ok()?;
        frame.shot_to_phys_checked(shot)
    };
    response
        .computer_calls
        .iter()
        .flat_map(|call| call.actions.iter())
        .map(|action| match action {
            Action::Click { x, y, .. } | Action::DoubleClick { x, y } => {
                look.snapshot.target_at(at(*x, *y)?)?.rect
            }
            Action::Scroll { x, y, .. } | Action::Move { x, y } => {
                look.snapshot.element_at(at(*x, *y)?)?.rect
            }
            // A slider's thumb: the pointable element where the drag starts.
            Action::Drag { path } => {
                let &(x, y) = path.first()?;
                look.snapshot.target_at(at(x, y)?)?.rect
            }
            Action::Type { .. } | Action::Keypress { .. } | Action::Wait | Action::Screenshot => {
                Some(look.snapshot.frame)
            }
            Action::Unknown { .. } => None,
        })
        .collect()
}

/// The person's clicks while a lesson runs: the low-level mouse hook, recognized into
/// clicks and double clicks, sent to the lesson. Dropping it removes the hook.
struct Clicks {
    _hook: mm_win32::mouse::MouseHook,
}

fn listen_for_clicks(
    started: Instant,
    inputs: mpsc::UnboundedSender<TeacherInput>,
) -> Option<Clicks> {
    let (events, received) = std::sync::mpsc::channel();
    let hook = mm_win32::mouse::start(events)
        .map_err(|error| eprintln!("teacher: mouse hook: {error}"))
        .ok()?;
    let (double_ms, cx, cy) = mm_win32::mouse::double_click_rules();
    std::thread::Builder::new()
        .name("mm-clicks".into())
        .spawn(move || {
            let mut recognizer = ClickRecognizer::new(double_ms, cx, cy);
            // Ends when the hook is dropped (its sender goes) or the lesson is gone.
            while let Ok(event) = received.recv() {
                let now_ms =
                    u64::try_from(event.time.saturating_duration_since(started).as_millis())
                        .unwrap_or(u64::MAX);
                let left = matches!(event.button, mm_win32::mouse::Button::Left);
                for input in recognizer.button(left, event.down, event.at, event.injected, now_ms) {
                    if inputs.send(input).is_err() {
                        return;
                    }
                }
            }
        })
        .ok()?;
    Some(Clicks { _hook: hook })
}

/// Takes a look on a blocking thread (the same rules as a "where" question).
async fn look(app: &AppHandle, generation: u64, own: Vec<isize>) -> Result<Look, LookFailure> {
    let looker = app.clone();
    tokio::task::spawn_blocking(move || {
        let wanted = || !looker.state::<AnswerStream>().is_cancelled(generation);
        take_look(&own, &wanted)
    })
    .await
    .unwrap_or(Err(LookFailure::Cancelled))
}

/// One lesson for `question`. Returns when the model says it is done, the teacher
/// refuses or gives up, the person stays idle too long, or the task is aborted (a new
/// press drops this future, its hook and its looks).
pub async fn teach(
    app: AppHandle,
    proxy: Proxy,
    phrases: Phrases,
    generation: u64,
    question: String,
    locale: String,
    own: Vec<isize>,
) {
    let stream = app.state::<AnswerStream>();
    let started = Instant::now();
    let now = || u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut current = match look(&app, generation, own.clone()).await {
        Ok(look) => Arc::new(look),
        Err(failure) => {
            eprintln!("teacher: look: {failure}");
            say_line(&stream, &phrases, &locale, generation, failure.line());
            stream.send(AnswerMsg::State {
                generation,
                state: "idle",
            });
            return;
        }
    };
    let (sent_width, sent_height) = current.redacted.frame.sent_size();
    let mut session = ComputerSession {
        model: ModelChoice::Quick,
        effort: ReasoningEffort::Minimal,
        sent_width,
        sent_height,
    };
    let mut teacher = Teacher::new(TeacherConfig::default());
    let (inputs, mut received) = mpsc::unbounded_channel();
    let _clicks = listen_for_clicks(started, inputs);
    let mut body = computer_request(
        session.model,
        session.effort,
        &question,
        &current.list,
        &current.redacted.image,
    );
    let mut sentence = 0u64;
    // The model's first point is sometimes off its target (live: a slider drag that
    // started 80 px to its left). Instead of guessing, the first turn is asked once more
    // with a little more reasoning; a second miss is "I can't find it" (invariant 5).
    let mut first_turn = true;
    let mut retried = false;
    let mut acknowledged: Vec<String> = Vec::new();
    let mut tick = tokio::time::interval(TICK);
    'turns: loop {
        // One model turn on the current look.
        let (mut pending, last_turn) = match proxy.respond(CallKind::Computer, &body, |_| {}).await
        {
            Ok(answer) => {
                let response = answer.response;
                let call_id = response
                    .computer_calls
                    .first()
                    .map(|call| call.call_id.clone());
                let turn = (response.id.clone(), call_id);
                let targets = targets_for(&current, &response);
                #[cfg(debug_assertions)]
                eprintln!("teacher: turn {response:?} targets {targets:?}");
                let frame = current.redacted.frame;
                let commands = teacher.input(TeacherInput::ModelTurn {
                    response,
                    frame,
                    targets,
                });
                (commands, Some(turn))
            }
            Err(error) => {
                eprintln!("teacher: model turn: {error}");
                (
                    teacher.input(TeacherInput::ModelFailed { now_ms: now() }),
                    None,
                )
            }
        };
        let missed = pending.iter().any(|command| {
            matches!(
                command,
                TeacherCommand::Refused(RefusalReason::TargetNotFound)
            )
        });
        if first_turn && missed && !retried {
            retried = true;
            teacher = Teacher::new(TeacherConfig::default());
            session.effort = ReasoningEffort::Low;
            body = computer_request(
                session.model,
                session.effort,
                &question,
                &current.list,
                &current.redacted.image,
            );
            continue 'turns;
        }
        first_turn = false;
        if let Some(pointing) = app.try_state::<Pointing>() {
            pointing.set_shared(generation, Arc::clone(&current));
        }
        let mut idle_since = Instant::now();
        loop {
            #[cfg(debug_assertions)]
            if !pending.is_empty() {
                eprintln!("teacher: commands {pending:?}");
            }
            for command in std::mem::take(&mut pending) {
                match command {
                    TeacherCommand::SpeakCommentary(text) => {
                        sentence += 1;
                        speak(&stream, &proxy, &locale, generation, sentence, text).await;
                    }
                    TeacherCommand::PointAt { rect, step, total } => {
                        let _ = show_target(
                            &app,
                            generation,
                            rect,
                            Some((u32::from(step), u32::from(total))),
                        );
                    }
                    TeacherCommand::Say(line) => {
                        if let Some(key) = line_key(line) {
                            say_line(&stream, &phrases, &locale, generation, key);
                        }
                    }
                    TeacherCommand::AcknowledgeSafetyChecks(ids) => acknowledged = ids,
                    TeacherCommand::UseModel(model) => session.model = model,
                    TeacherCommand::RequestScreenshot => {
                        let Some((response_id, Some(call_id))) = last_turn.clone() else {
                            finish(&app, &stream, generation);
                            return;
                        };
                        match look(&app, generation, own.clone()).await {
                            Ok(next) => current = Arc::new(next),
                            Err(failure) => {
                                eprintln!("teacher: look: {failure}");
                                say_line(&stream, &phrases, &locale, generation, failure.line());
                                finish(&app, &stream, generation);
                                return;
                            }
                        }
                        body = computer_followup(
                            session,
                            &response_id,
                            &call_id,
                            &acknowledged,
                            &current.redacted.image,
                        );
                        acknowledged.clear();
                        continue 'turns;
                    }
                    TeacherCommand::Finished | TeacherCommand::Refused(_) => {
                        finish(&app, &stream, generation);
                        return;
                    }
                }
            }
            if idle_since.elapsed() > IDLE_LIMIT {
                finish(&app, &stream, generation);
                return;
            }
            tokio::select! {
                input = received.recv() => {
                    let Some(input) = input else { continue };
                    idle_since = Instant::now();
                    pending = teacher.input(input);
                }
                _ = tick.tick() => pending = teacher.input(TeacherInput::Tick { now_ms: now() }),
            }
        }
    }
}

/// A commentary line as the next sentence of the lesson's timeline, with its speech.
async fn speak(
    stream: &AnswerStream,
    proxy: &Proxy,
    locale: &str,
    generation: u64,
    id: u64,
    text: String,
) {
    let say = Say {
        id,
        text: text.clone(),
        bound: Vec::new(),
    };
    stream.send(AnswerMsg::Item {
        generation,
        item: Item::Say(say),
    });
    match proxy.speak(voice_of(locale), &text).await {
        Ok(speech) => stream.send_wav(generation, id, &speech.wav),
        Err(error) => {
            eprintln!("teacher: speech synthesis: {error}");
            stream.end_sentence(generation, id);
        }
    }
}

/// The lesson's timeline ends and the beam goes out.
fn finish(app: &AppHandle, stream: &AnswerStream, generation: u64) {
    stream.send(AnswerMsg::Item {
        generation,
        item: Item::End,
    });
    stop_pointing(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_the_teacher_says_has_a_locale_key_or_its_commentary() {
        for line in [
            LineKey::WrongPlace,
            LineKey::RightClick,
            LineKey::CantFind,
            LineKey::Done,
            LineKey::NetworkError,
            LineKey::TooManyTurns,
            LineKey::WontPress,
        ] {
            let key = line_key(line).unwrap();
            let en: serde_json::Value =
                serde_json::from_str(include_str!("../../locales/en.json")).unwrap();
            assert!(en.get(key).is_some(), "{key} missing");
        }
        assert_eq!(line_key(LineKey::StepN), None);
    }
}
