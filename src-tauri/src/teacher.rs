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

use mm_core::api::intent::{asks_do_it, parse_yes_no};
use mm_core::api::request::{
    ComputerSession, ModelChoice, ReasoningEffort, computer_followup, computer_request,
};
use mm_core::api::response::{Action, ParsedResponse};
use mm_core::geom::{PhysRect, ShotPoint};
use mm_core::matescript::{Item, Say};
use mm_core::redaction::{overlaps_any, point_in_any};
use mm_core::teacher::{
    ChordRecognizer, ClickRecognizer, LineKey, RefusalReason, Teacher, TeacherCommand,
    TeacherConfig, TeacherInput,
};
use mm_platform::{HumanConfirmation, InputAction, InputInjector, MouseButton};
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
    pub text: String,
    pub taken: oneshot::Sender<bool>,
}

/// Check the whole snapshot ancestry, including the window's title, before offering.
fn pressable(snapshot: &mm_platform::UiSnapshot, rect: PhysRect) -> bool {
    if snapshot.truncated {
        return false;
    }
    let Some(mut element) = snapshot.target_at(rect.center()) else {
        return false;
    };
    let hidden = snapshot.hidden_ids();
    // Popup roots have no link to the target window; its title still guards the click.
    let Some(window) = snapshot
        .elements
        .iter()
        .find(|element| element.parent.is_none() && element.control_type == "Window")
    else {
        return false;
    };
    if hidden.contains(&window.id)
        || mm_core::privacy::forbidden(&window.name, &window.control_type)
    {
        return false;
    }
    let mut named = false;
    for level in 0..snapshot.elements.len() {
        if hidden.contains(&element.id)
            || mm_core::privacy::forbidden(&element.name, &element.control_type)
        {
            return false;
        }
        if level == 0 || !["Window", "Pane", "Document"].contains(&element.control_type.as_str()) {
            named |= !element.name.trim().is_empty();
        }
        if element.control_type == "Window"
            || (element.parent.is_none()
                && ["Menu", "List", "Pane"].contains(&element.control_type.as_str()))
        {
            return named;
        }
        let Some(parent) = element
            .parent
            .as_deref()
            .and_then(|id| snapshot.element(id))
        else {
            return false;
        };
        element = parent;
    }
    false // Missing roots and parent cycles fail closed.
}

/// The locale key of a teacher line; None for a line that needs words the model gives
/// (the step's own commentary says it).
/// Praise for a step done, in turn: the same words after every click would sound canned.
const PRAISE: [&str; 4] = [
    "robot.right_click",
    "robot.right_click_2",
    "robot.right_click_3",
    "robot.right_click_4",
];

fn line_key(line: LineKey, steps_done: &mut usize) -> Option<&'static str> {
    match line {
        LineKey::WrongPlace => Some("robot.wrong_place"),
        LineKey::RightClick => {
            let key = PRAISE[*steps_done % PRAISE.len()];
            *steps_done += 1;
            Some(key)
        }
        LineKey::CantFind => Some("robot.cant_find"),
        LineKey::StepN => None,
        LineKey::Done => Some("robot.done"),
        LineKey::NetworkError => Some("robot.network_error"),
        LineKey::TooManyTurns => Some("robot.too_many_turns"),
        LineKey::WontPress => Some("robot.wont_press"),
        LineKey::DoItForMe => Some("robot.do_it_for_me"),
        LineKey::Pressing => Some("robot.pressing"),
        LineKey::PressCancelled => Some("robot.press_cancelled"),
        LineKey::PressFailed => Some("robot.press_failed"),
    }
}

/// The rectangle each action of a turn is about, in the order the teacher reads them: the
/// pointable element under a click, the element under a scroll or move, the whole look
/// for typing, keys and waiting; None for what cannot be placed (the teacher refuses),
/// including anything in or touching a painted area: the model saw black there.
fn targets_for(look: &Look, response: &ParsedResponse) -> Vec<Option<PhysRect>> {
    let frame = look.redacted.frame;
    let at = |x: i64, y: i64| {
        let shot = ShotPoint::new(x as f64 + 0.5, y as f64 + 0.5).ok()?;
        let phys = frame.shot_to_phys_checked(shot)?;
        (!point_in_any(phys, &look.painted)).then_some(phys)
    };
    let visible = |rect: PhysRect| (!overlaps_any(rect, &look.painted)).then_some(rect);
    response
        .computer_calls
        .iter()
        .flat_map(|call| call.actions.iter())
        .map(|action| match action {
            Action::Click { x, y, .. } | Action::DoubleClick { x, y } => {
                visible(look.snapshot.target_at(at(*x, *y)?)?.rect?)
            }
            Action::Scroll { x, y, .. } | Action::Move { x, y } => {
                visible(look.snapshot.element_at(at(*x, *y)?)?.rect?)
            }
            // A slider's thumb: the pointable element where the drag starts.
            Action::Drag { path } => {
                let &(x, y) = path.first()?;
                let &(end_x, end_y) = path.last()?;
                at(end_x, end_y)?;
                visible(look.snapshot.target_at(at(x, y)?)?.rect?)
            }
            // No privacy-safe ValueChanged source exists yet. Refuse type steps
            // even when the live keyboard subscription enables keypress steps.
            Action::Type { .. } => None,
            Action::Keypress { .. } | Action::Wait | Action::Screenshot => {
                Some(look.snapshot.frame)
            }
            Action::Unknown { .. } => None,
        })
        .collect()
}

/// Start the lesson with the selected model.
fn lesson_session(model: ModelChoice, (sent_width, sent_height): (u32, u32)) -> ComputerSession {
    ComputerSession {
        model,
        effort: ReasoningEffort::Minimal,
        sent_width,
        sent_height,
    }
}

fn fell_back(asked: ModelChoice, served: Option<&str>) -> bool {
    asked == ModelChoice::Thorough && served.is_some_and(|model| model.starts_with("muse-spark"))
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

/// The lesson owns the subscription, so aborting its future disconnects the worker.
struct Keys {
    _subscription: crate::ptt::KeyTapGuard,
}

fn listen_for_keys(started: Instant, inputs: mpsc::UnboundedSender<TeacherInput>) -> Option<Keys> {
    let (subscription, received) = crate::ptt::subscribe_keys()?;
    std::thread::Builder::new()
        .name("mm-keys".into())
        .spawn(move || {
            let mut recognizer = ChordRecognizer::new();
            while let Ok(event) = received.recv() {
                if event.at < started {
                    continue;
                }
                let now_ms =
                    u64::try_from(event.at.duration_since(started).as_millis()).unwrap_or(u64::MAX);
                if let Some(input) = recognizer.key(event.vk, event.down, event.injected, now_ms)
                    && inputs.send(input).is_err()
                {
                    return;
                }
            }
        })
        .ok()?;
    Some(Keys {
        _subscription: subscription,
    })
}

/// Takes a look on a blocking thread (the same rules as a "where" question).
async fn look(
    app: &AppHandle,
    generation: u64,
    own: Vec<isize>,
    target: Option<mm_platform::TargetWindow>,
) -> Result<Look, LookFailure> {
    let looker = app.clone();
    tokio::task::spawn_blocking(move || {
        // Mate may still be flying: the copy waits until he is still.
        if let Some(flight) = looker.try_state::<crate::flight::Flight>() {
            flight.wait_idle(Duration::from_secs(1));
        }
        let wanted = || !looker.state::<AnswerStream>().is_cancelled(generation);
        // Mate's eyes sweep while he copies the window (the robot's "looking" state).
        looker.state::<AnswerStream>().send(AnswerMsg::State {
            generation,
            state: "looking",
        });
        let look = crate::screen::take_look_at(&own, &wanted, target);
        looker.state::<AnswerStream>().send(AnswerMsg::State {
            generation,
            state: "thinking",
        });
        look
    })
    .await
    .unwrap_or(Err(LookFailure::Cancelled))
}

/// One lesson for `question`. Returns when the model says it is done, the teacher
/// refuses or gives up, the person stays idle too long, or the task is aborted (a new
/// unrelated question drops this future, its hook and its looks).
#[expect(
    clippy::too_many_arguments,
    reason = "the answer path's own state, handed over once"
)]
pub(crate) async fn teach(
    app: AppHandle,
    proxy: Proxy,
    phrases: Phrases,
    generation: u64,
    question: String,
    locale: String,
    own: Vec<isize>,
    mut words: mpsc::UnboundedReceiver<Words>,
    speech_epoch: Arc<AtomicU64>,
    target: Option<mm_platform::TargetWindow>,
) {
    let model = ModelChoice::from(crate::settings::model(&app));
    let stream = app.state::<AnswerStream>();
    let started = Instant::now();
    let now = || u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut current = match look(&app, generation, own.clone(), target).await {
        Ok(look) => Arc::new(look),
        Err(failure) => {
            if stream.is_cancelled(generation) {
                return;
            }
            eprintln!("teacher: look: {failure}");
            // Like voice.rs: no timeline was started, so no End (it would cut the line's
            // bubble short).
            say_line(&stream, &phrases, &locale, generation, failure.line());
            stream.send(AnswerMsg::State {
                generation,
                state: "idle",
            });
            return;
        }
    };
    let mut session = lesson_session(model, current.redacted.frame.sent_size());
    let (inputs, mut received) = mpsc::unbounded_channel();
    let keys = listen_for_keys(started, inputs.clone());
    let config = TeacherConfig {
        configured_model: model,
        auto_escalate: true,
        do_it_for_me: true,
        keyboard_steps: keys.is_some(),
        ..TeacherConfig::default()
    };
    let mut teacher = Teacher::new(config.clone());
    let Some(_clicks) = listen_for_clicks(started, inputs) else {
        // Without the hook no step could ever complete.
        say_line(&stream, &phrases, &locale, generation, "robot.cant_look");
        finish(&app, &stream, generation);
        return;
    };
    let mut body = computer_request(
        session.model,
        session.effort,
        &question,
        &current.list,
        &current.redacted.image,
    );
    let mut sentence = 0u64;
    let mut steps_done = 0usize;
    // The commentary lines in order: the steps of the lesson's card.
    let mut card_steps: Vec<String> = Vec::new();
    // The model's first point is sometimes off its target (live: a slider drag that
    // started 80 px to its left). Instead of guessing, the first turn is asked once more
    // with a little more reasoning; a second miss is "I can't find it" (invariant 5).
    let mut first_turn = true;
    let mut retried = false;
    let mut acknowledged: Vec<String> = Vec::new();
    let mut tick = tokio::time::interval(TICK);
    let mut offered_at: Option<Instant> = None;
    let mut shown_model = None;
    'turns: loop {
        let cancelled = || stream.is_cancelled(generation);
        // One model turn on the current look. A failed turn (an error, a cut or unfinished
        // answer, or more than one computer call: the prompt asks for one) is asked again;
        // the teacher refuses on the second failure.
        let (mut pending, last_turn, failed) =
            match proxy.respond(CallKind::Computer, &body, |_| {}).await {
                Ok(answer) => {
                    let served = model_chip(answer.model.as_deref());
                    if served != shown_model {
                        stream.send(AnswerMsg::Model {
                            generation,
                            model: served,
                        });
                        shown_model = served;
                    }
                    if fell_back(session.model, answer.model.as_deref()) {
                        for command in teacher.input(TeacherInput::WonderUnavailable) {
                            if let TeacherCommand::UseModel(model) = command {
                                session.model = model;
                            }
                        }
                        // A failed answer is retried with Quick too.
                        body["model"] = serde_json::json!(session.model);
                    }
                    let response = answer.response;
                    let failed = response.status != "completed"
                        || response.incomplete_reason.is_some()
                        || response.computer_calls.len() > 1;
                    if failed {
                        let commands = teacher.input(TeacherInput::ModelFailed { now_ms: now() });
                        (commands, None, true)
                    } else {
                        let call_id = response
                            .computer_calls
                            .first()
                            .map(|call| call.call_id.clone());
                        let turn = (response.id.clone(), call_id);
                        let mut targets = targets_for(&current, &response);
                        outline_unlisted(&proxy, &current, &response, &mut targets).await;
                        #[cfg(debug_assertions)]
                        eprintln!("teacher: turn {response:?} targets {targets:?}");
                        let frame = current.redacted.frame;
                        let commands = teacher.input(TeacherInput::ModelTurn {
                            response,
                            frame,
                            targets,
                        });
                        (commands, Some(turn), false)
                    }
                }
                Err(error) => {
                    eprintln!("teacher: model turn: {error}");
                    let commands = teacher.input(TeacherInput::ModelFailed { now_ms: now() });
                    (commands, None, true)
                }
            };
        if cancelled() {
            return;
        }
        let refused = pending.iter().any(|command| {
            matches!(
                command,
                TeacherCommand::Refused(_) | TeacherCommand::Finished
            )
        });
        if failed && !refused {
            // Meta's computer use sometimes answers 500 twice in a row (seen 10-28): a
            // short pause gives the second try a better chance.
            tokio::time::sleep(RETRY_PAUSE).await;
            continue 'turns;
        }
        let missed = pending.iter().any(|command| {
            matches!(
                command,
                TeacherCommand::Refused(RefusalReason::TargetNotFound)
            )
        });
        if first_turn && missed && !retried {
            retried = true;
            teacher = Teacher::new(config.clone());
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
        let mut progress = Instant::now();
        // Clicks and chords from before the current step was shown never complete it
        // (invariant 4: input must not confirm a step the person never saw).
        let mut pointed_ms = now();
        loop {
            #[cfg(debug_assertions)]
            if !pending.is_empty() {
                eprintln!("teacher: commands {pending:?}");
            }
            let mut ended = false;
            // Only a lesson the model finished, nothing refused on the way, gets a card.
            let mut finished = false;
            let mut any_refused = false;
            // The model's own last words ("All done!") make the cached "done" line a repeat.
            let mut spoke = false;
            // The last turn's words close the lesson ("All done!"): no step of its card.
            let closing = pending
                .iter()
                .any(|command| matches!(command, TeacherCommand::Finished));
            for command in std::mem::take(&mut pending) {
                if cancelled() {
                    return;
                }
                match command {
                    TeacherCommand::SpeakCommentary(text) => {
                        // The prompt allows a leading `<lang ..><mood ..>`: never spoken.
                        let text = crate::card::without_tags(&text).trim().to_owned();
                        if !closing
                            && card_steps.len() < crate::card::MAX_STEPS
                            && !text.trim().is_empty()
                        {
                            card_steps.push(text.clone());
                        }
                        sentence += 1;
                        speak(
                            &stream,
                            &proxy,
                            &locale,
                            generation,
                            sentence,
                            text,
                            &speech_epoch,
                        )
                        .await;
                        spoke = true;
                    }
                    TeacherCommand::PointAt { rect, step, total } => {
                        progress = Instant::now();
                        // The picture must still be true where the ring goes.
                        let true_picture = mm_win32::window::still_shown(
                            current.snapshot.target,
                            current.window_frame,
                        );
                        let shown = true_picture
                            && show_target(
                                &app,
                                generation,
                                rect,
                                Some((u32::from(step), u32::from(total))),
                            )
                            .is_some();
                        pointed_ms = now();
                        if !shown {
                            // The last step's ring would stay over whatever is there now.
                            let _ = crate::highlight::clear(&app);
                        }
                        if !true_picture {
                            pending.extend(
                                teacher.input(TeacherInput::ScreenChangedUnexpectedly {
                                    now_ms: now(),
                                }),
                            );
                        } else if !shown {
                            say_line(&stream, &phrases, &locale, generation, "robot.cant_find");
                            ended = true;
                        }
                    }
                    TeacherCommand::Say(LineKey::Done) if spoke => {}
                    TeacherCommand::Say(line) => {
                        if let Some(key) = line_key(line, &mut steps_done) {
                            say_line(&stream, &phrases, &locale, generation, key);
                        }
                    }
                    TeacherCommand::OfferDoIt { rect } => {
                        if pressable(&current.snapshot, rect) {
                            say_line(&stream, &phrases, &locale, generation, "robot.do_it_for_me");
                            offered_at = Some(Instant::now());
                        } else {
                            offered_at = None;
                            pending.extend(
                                teacher.input(TeacherInput::DoItUnavailable { now_ms: now() }),
                            );
                        }
                    }
                    TeacherCommand::PerformClick { at, rect } => {
                        offered_at = None;
                        let target = current.snapshot.target;
                        let frame = current.window_frame;
                        let region = current.snapshot.frame;
                        let allowed = pressable(&current.snapshot, rect);
                        let ok = tokio::task::spawn_blocking(move || {
                            // The window unmoved and its popups (an offered menu item's menu)
                            // still where they were: the region of the look is unchanged.
                            let unchanged = mm_win32::window::still_shown(target, frame)
                                && mm_win32::window::foreground_target(&[]).is_some_and(|front| {
                                    front.window == target && front.region == region
                                });
                            if !allowed || !unchanged {
                                eprintln!("teacher: confirmed click: target changed or blocked");
                                return false;
                            }
                            let action = InputAction::Click {
                                at,
                                button: MouseButton::Left,
                            };
                            // This command can only follow the person's yes to the live offer.
                            let confirmation =
                                HumanConfirmation::given_by_user(target, action.clone());
                            mm_win32::input::Win32Injector
                                .execute_verified(target, &action, Some(confirmation))
                                .map_err(|_| {
                                    eprintln!("teacher: confirmed click: injection refused")
                                })
                                .is_ok()
                        })
                        .await
                        .unwrap_or_else(|_| {
                            eprintln!("teacher: confirmed click: worker failed");
                            false
                        });
                        pending.extend(teacher.input(TeacherInput::DoItDone { ok, now_ms: now() }));
                    }
                    TeacherCommand::AcknowledgeSafetyChecks(ids) => acknowledged = ids,
                    TeacherCommand::UseModel(model) => {
                        session.model = model;
                        if model == ModelChoice::Thorough {
                            say_line(
                                &stream,
                                &phrases,
                                &locale,
                                generation,
                                "robot.thinking_harder",
                            );
                        }
                    }
                    TeacherCommand::RequestScreenshot => {
                        offered_at = None;
                        let Some((response_id, Some(call_id))) = last_turn.clone() else {
                            finish(&app, &stream, generation);
                            return;
                        };
                        let fallback = target.and_then(|_| {
                            mm_win32::window::foreground_target(&[])
                                .is_none()
                                .then_some(current.snapshot.target)
                        });
                        match look(&app, generation, own.clone(), fallback).await {
                            Ok(next) => current = Arc::new(next),
                            Err(failure) => {
                                if cancelled() {
                                    return;
                                }
                                eprintln!("teacher: look: {failure}");
                                say_line(&stream, &phrases, &locale, generation, failure.line());
                                finish(&app, &stream, generation);
                                return;
                            }
                        }
                        // The model answers in pixels of the image it was last sent: each
                        // turn's own frame maps them (D48), so the size may follow the window.
                        (session.sent_width, session.sent_height) =
                            current.redacted.frame.sent_size();
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
                    // The lines of the same batch ("I don't press that", "All done") are
                    // still said before the lesson ends.
                    TeacherCommand::Finished => {
                        ended = true;
                        finished = true;
                    }
                    TeacherCommand::Refused(_) => {
                        ended = true;
                        any_refused = true;
                    }
                }
            }
            if ended {
                if finished && !any_refused && !cancelled() {
                    // Drawn beside the lesson, which ends now (card.rs).
                    tauri::async_runtime::spawn(crate::card::make(
                        app.clone(),
                        question.clone(),
                        std::mem::take(&mut card_steps),
                    ));
                }
                finish(&app, &stream, generation);
                return;
            }
            // Process commands produced by an offer or injection before waiting again.
            if !pending.is_empty() {
                continue;
            }
            if progress.elapsed() > IDLE_LIMIT || started.elapsed() > LESSON_LIMIT {
                say_line(&stream, &phrases, &locale, generation, "robot.sleep");
                finish(&app, &stream, generation);
                return;
            }
            tokio::select! {
                Some(words) = words.recv() => {
                    if words.taken.is_closed() {
                        continue;
                    }
                    expire_offer(&mut teacher, &mut offered_at, now());
                    let input = if offered_at.is_some() {
                        parse_yes_no(&words.text).map(|yes| TeacherInput::DoItAnswer {
                            yes,
                            now_ms: now(),
                        })
                    } else {
                        None
                    }.or_else(|| asks_do_it(&words.text).then(|| {
                        TeacherInput::DoItRequested { now_ms: now() }
                    }));
                    let taken = input.is_some();
                    // A superseded recognition task cannot authorize a click.
                    if words.taken.send(taken).is_err() {
                        continue;
                    }
                    if let Some(input) = input {
                        if matches!(input, TeacherInput::DoItAnswer { .. }) {
                            offered_at = None;
                        }
                        pending = teacher.input(input);
                    }
                }
                input = received.recv() => {
                    let Some(input) = input else {
                        // The click thread is gone: no step can complete any more.
                        finish(&app, &stream, generation);
                        return;
                    };
                    let stale = match &input {
                        TeacherInput::KeyChord { now_ms, .. } => {
                            *now_ms < pointed_ms
                                || !mm_win32::window::foreground_target(&[])
                                    .is_some_and(|front| front.window == current.snapshot.target)
                        }
                        TeacherInput::Click { now_ms, .. }
                        | TeacherInput::DoubleClick { now_ms, .. } => {
                            *now_ms < pointed_ms
                        }
                        _ => false,
                    };
                    if !stale {
                        pending = teacher.input(input);
                        if pending.iter().any(|command| matches!(command,
                            TeacherCommand::PointAt { .. }
                            | TeacherCommand::RequestScreenshot
                            | TeacherCommand::Say(LineKey::RightClick)
                        )) {
                            // A wrong click may re-point the same step while its offer is live.
                            if !pending.iter().any(|command| {
                                matches!(command, TeacherCommand::Say(LineKey::WrongPlace))
                            }) {
                                offered_at = None;
                            }
                        }
                    }
                }
                _ = tick.tick() => {
                    expire_offer(&mut teacher, &mut offered_at, now());
                    pending = teacher.input(TeacherInput::Tick { now_ms: now() });
                }
            }
        }
    }
}

fn expire_offer(teacher: &mut Teacher, offered_at: &mut Option<Instant>, now_ms: u64) {
    if offered_at.is_some_and(|at| at.elapsed() >= OFFER_LIMIT) {
        *offered_at = None;
        // Reset Awaiting without speaking. A later explicit request can offer again.
        let _ = teacher.input(TeacherInput::DoItUnavailable { now_ms });
    }
}

/// Resolve unlisted clicks live after a cut walk, then try SAM's outline when allowed.
/// Otherwise the step stays unplaceable and the teacher refuses (invariant 5).
async fn outline_unlisted(
    proxy: &Proxy,
    look: &Arc<Look>,
    response: &ParsedResponse,
    targets: &mut [Option<PhysRect>],
) {
    let actions = response
        .computer_calls
        .iter()
        .flat_map(|call| call.actions.iter());
    // All at once: a turn with several such clicks waits for the slowest, not the sum.
    let mut asked = Vec::new();
    for (index, (action, target)) in actions.zip(targets.iter()).enumerate() {
        let (Action::Click { x, y, .. } | Action::DoubleClick { x, y }) = action else {
            continue;
        };
        let (Ok(x), Ok(y)) = (u32::try_from(*x), u32::try_from(*y)) else {
            continue;
        };
        if target.is_none() && (look.cut || crate::sam::outline_allowed(look, x, y)) {
            let (proxy, look) = (proxy.clone(), Arc::clone(look));
            let task = tokio::spawn(async move {
                if look.cut
                    && let Some(phys) = ShotPoint::new(f64::from(x) + 0.5, f64::from(y) + 0.5)
                        .ok()
                        .and_then(|shot| look.redacted.frame.shot_to_phys_checked(shot))
                    && !point_in_any(phys, &look.painted)
                {
                    let element = tokio::task::spawn_blocking(move || {
                        mm_win32::uia::UiaWalker::new().ok()?.element_at_point(phys)
                    })
                    .await
                    .ok()
                    .flatten();
                    if let Some(rect) = look
                        .resolve_live(element)
                        .filter(|rect| rect.contains(phys))
                    {
                        return Some(rect);
                    }
                }
                if crate::sam::outline_allowed(&look, x, y) {
                    crate::sam::sam_outline(&proxy, &look, x, y).await
                } else {
                    None
                }
            });
            asked.push((index, task));
        }
    }
    for (index, task) in asked {
        targets[index] = task.await.ok().flatten();
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
    speech_epoch: &AtomicU64,
) {
    let epoch = speech_epoch.load(Ordering::Acquire);
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
        Ok(speech) if speech_epoch.load(Ordering::Acquire) == epoch => {
            stream.send_wav(generation, id, &speech.wav);
        }
        Ok(_) => stream.end_sentence(generation, id),
        Err(error) => {
            eprintln!("teacher: speech synthesis: {error}");
            stream.end_sentence(generation, id);
        }
    }
}

/// The lesson's timeline ends and the beam goes out (unless a newer press took over).
fn finish(app: &AppHandle, stream: &AnswerStream, generation: u64) {
    if stream.is_cancelled(generation) {
        return;
    }
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
    fn only_wonder_served_by_spark_is_a_fallback() {
        assert!(fell_back(ModelChoice::Thorough, Some("muse-spark-1.3")));
        assert!(fell_back(ModelChoice::Thorough, Some("muse-spark")));
        assert!(!fell_back(ModelChoice::Quick, Some("muse-spark-1.3")));
        assert!(!fell_back(ModelChoice::Thorough, Some("muse-wonder-1.0")));
        assert!(!fell_back(ModelChoice::Thorough, Some("other-model")));
        assert!(!fell_back(ModelChoice::Thorough, Some("")));
        assert!(!fell_back(ModelChoice::Thorough, None));
    }

    fn offer_snapshot() -> mm_platform::UiSnapshot {
        let frame = PhysRect::new(0, 0, 100, 100).unwrap();
        let element =
            |id: &str, parent: Option<&str>, kind: &str, name: &str, rect| mm_platform::UiElement {
                id: id.into(),
                parent: parent.map(str::to_owned),
                control_type: kind.into(),
                name: name.into(),
                rect: Some(rect),
                interactive: kind == "Button",
                enabled: true,
                offscreen: false,
                is_password: false,
                password_like: false,
            };
        mm_platform::UiSnapshot {
            target: mm_platform::TargetWindow {
                handle: 1,
                process_id: 1,
            },
            frame,
            truncated: false,
            elements: vec![
                element("window", None, "Window", "Settings", frame),
                element("pane", Some("window"), "Pane", "Options", frame),
                element(
                    "button",
                    Some("pane"),
                    "Button",
                    "Open",
                    PhysRect::new(10, 10, 30, 30).unwrap(),
                ),
            ],
        }
    }

    fn prepared_look(painted: &[PhysRect]) -> Look {
        let capture = mm_platform::CapturedImage::new(100, 100, vec![200; 100 * 100 * 4]).unwrap();
        crate::screen::prepare(offer_snapshot(), &capture, painted).unwrap()
    }

    #[test]
    fn selected_model_survives_retries_and_followups() {
        let look = prepared_look(&[]);
        for selected in [
            crate::settings::Model::Quick,
            crate::settings::Model::Thorough,
        ] {
            let model = ModelChoice::from(selected);
            let mut session = lesson_session(model, look.redacted.frame.sent_size());
            let expected = serde_json::to_value(model).unwrap();
            for effort in [ReasoningEffort::Minimal, ReasoningEffort::Low] {
                session.effort = effort;
                let first = computer_request(
                    session.model,
                    session.effort,
                    "Open options",
                    &look.list,
                    &look.redacted.image,
                );
                let next =
                    computer_followup(session, "response", "call", &[], &look.redacted.image);
                assert_eq!(first["model"], expected);
                assert_eq!(next["model"], expected);
            }
        }
    }

    #[test]
    fn drags_refuse_painted_starts_and_ends() {
        let response = |path| ParsedResponse {
            id: "response".into(),
            model: String::new(),
            status: "completed".into(),
            commentary: Vec::new(),
            computer_calls: vec![mm_core::api::response::ComputerCall {
                call_id: "call".into(),
                actions: vec![Action::Drag { path }],
                safety_checks: Vec::new(),
                replay_receipt: None,
            }],
            usage: None,
            incomplete_reason: None,
            reasoning_items: 0,
        };
        let painted = PhysRect::new(60, 60, 80, 80).unwrap();
        let look = prepared_look(&[painted]);
        let target = look.snapshot.elements[2].rect;
        assert_eq!(
            targets_for(&look, &response(vec![(20, 20), (50, 50)])),
            vec![target]
        );
        for path in [
            vec![(20, 20), (60, 60)],
            vec![(60, 60), (20, 20)],
            vec![(20, 20), (100, 50)],
            Vec::new(),
        ] {
            assert_eq!(targets_for(&look, &response(path)), vec![None]);
        }
    }

    #[test]
    fn popup_roots_still_check_the_target_window_title() {
        for kind in ["Menu", "List", "Pane"] {
            let mut snapshot = offer_snapshot();
            let rect = snapshot.elements[2].rect.unwrap();
            snapshot.elements[1].parent = None;
            snapshot.elements[1].control_type = kind.into();
            snapshot.elements[2].control_type = "MenuItem".into();
            assert!(pressable(&snapshot, rect), "{kind}");
            snapshot.elements[0].name = "Delete file?".into();
            assert!(!pressable(&snapshot, rect));
            snapshot.elements[0].name = "Settings".into();
            snapshot.elements[1].name = "Payment".into();
            assert!(!pressable(&snapshot, rect));
            snapshot.elements[1].name.clear();
            snapshot.elements[2].name.clear();
            assert!(!pressable(&snapshot, rect));
            snapshot.elements[2].name = "Open".into();
            snapshot.elements[1].is_password = true;
            assert!(!pressable(&snapshot, rect));
        }
    }

    #[test]
    fn popup_roots_need_a_target_window_and_complete_snapshot() {
        let mut snapshot = offer_snapshot();
        let rect = snapshot.elements[2].rect.unwrap();
        snapshot.elements[1].parent = None;
        snapshot.elements[1].control_type = "Menu".into();
        snapshot.truncated = true;
        assert!(!pressable(&snapshot, rect));
        snapshot.truncated = false;
        snapshot.elements[0].control_type = "Pane".into();
        assert!(!pressable(&snapshot, rect));
        snapshot.elements[0].control_type = "Window".into();
        snapshot.elements[1].control_type = "Custom".into();
        assert!(!pressable(&snapshot, rect));
    }

    #[test]
    fn offers_check_forbidden_and_hidden_ancestors() {
        let mut snapshot = offer_snapshot();
        let rect = snapshot.elements[2].rect.unwrap();
        assert!(pressable(&snapshot, rect));
        snapshot.elements[0].name = "Delete file?".into();
        assert!(!pressable(&snapshot, rect));
        snapshot.elements[0].name = "Settings".into();
        snapshot.elements[1].is_password = true;
        assert!(!pressable(&snapshot, rect));
    }

    #[test]
    fn container_names_do_not_identify_an_unnamed_button() {
        let mut snapshot = offer_snapshot();
        let rect = snapshot.elements[2].rect.unwrap();
        snapshot.elements[2].name.clear();
        assert!(!pressable(&snapshot, rect));
        snapshot.elements[1].control_type = "Button".into();
        assert!(pressable(&snapshot, rect));
    }

    #[test]
    fn incomplete_and_cyclic_ancestry_fail_closed() {
        let mut snapshot = offer_snapshot();
        let rect = snapshot.elements[2].rect.unwrap();
        snapshot.elements[1].parent = Some("missing".into());
        assert!(!pressable(&snapshot, rect));
        snapshot.elements[1].parent = Some("button".into());
        assert!(!pressable(&snapshot, rect));
    }

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
            LineKey::DoItForMe,
            LineKey::Pressing,
            LineKey::PressCancelled,
            LineKey::PressFailed,
        ] {
            let key = line_key(line, &mut 0).unwrap();
            let en: serde_json::Value =
                serde_json::from_str(include_str!("../../locales/en.json")).unwrap();
            assert!(en.get(key).is_some(), "{key} missing");
        }
        assert_eq!(line_key(LineKey::StepN, &mut 0), None);
    }

    #[test]
    fn praise_rotates_through_every_line_in_both_languages() {
        let en: serde_json::Value =
            serde_json::from_str(include_str!("../../locales/en.json")).unwrap();
        let ru: serde_json::Value =
            serde_json::from_str(include_str!("../../locales/ru.json")).unwrap();
        let mut done = 0;
        let said: Vec<_> = (0..5)
            .map(|_| line_key(LineKey::RightClick, &mut done).unwrap())
            .collect();
        assert_eq!(said[0], "robot.right_click");
        assert_eq!(said[4], "robot.right_click");
        for key in &said[..4] {
            assert!(
                en.get(key).is_some() && ru.get(key).is_some(),
                "{key} missing"
            );
        }
        assert_eq!(done, 5);
    }
}
