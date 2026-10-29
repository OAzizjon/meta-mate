use super::*;

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
        let confirm = |target| crate::screen::confirm_look(&looker, generation, target, &wanted);
        let look = crate::screen::take_look_at(&own, &wanted, target, &confirm);
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
    let mut asked_to_focus = false;
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
    let lesson_window = current.snapshot.target;
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
    let mut prompt_epoch = 0;
    let mut audio_until = Instant::now();
    let mut audio_known = true;
    let mut confirmation_offer = None;
    let mut confirmed_click = None;
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
        let existing_windows = mm_win32::window::existing_windows();
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
                if offered_at.is_some() && mm_core::teacher::closes_confirmation(&command) {
                    offered_at = None;
                    confirmation_offer = None;
                    // Reset Awaiting without adding another spoken line.
                    let _ = teacher.input(TeacherInput::DoItAnswer {
                        yes: false,
                        now_ms: now(),
                    });
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
                        let duration = speak(
                            &stream,
                            &proxy,
                            &locale,
                            generation,
                            sentence,
                            text,
                            &speech_epoch,
                        )
                        .await;
                        if let Some(duration) = duration {
                            audio_until = audio_until.max(Instant::now()) + duration;
                        } else {
                            audio_known = false;
                        }
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
                            if let Some(wav) = crate::voice::phrase(&phrases, &stream.locale(), key)
                            {
                                if let Some(duration) = wav_duration(&wav) {
                                    audio_until = audio_until.max(Instant::now()) + duration;
                                } else {
                                    audio_known = false;
                                }
                            }
                        }
                    }
                    TeacherCommand::OfferDoIt { rect } => {
                        confirmation_offer = None;
                        confirmed_click = None;
                        let target = current.snapshot.target;
                        let at = rect.center();
                        let offer = if pressable(&current.snapshot, rect) && audio_known {
                            tokio::task::spawn_blocking(move || {
                                let action = InputAction::Click {
                                    at,
                                    button: MouseButton::Left,
                                };
                                mm_win32::input::Win32Injector.ask_confirmation(target, &action)
                            })
                            .await
                            .ok()
                            .and_then(Result::ok)
                        } else {
                            None
                        };
                        if let Some(offer) = offer.filter(|offer| {
                            !stream.is_cancelled(generation)
                                && current.snapshot.target_at(at).is_some_and(|expected| {
                                    expected.rect.is_some_and(|bounds| {
                                        offer.element().matches_target(
                                            &expected.name,
                                            &expected.control_type,
                                            bounds,
                                        )
                                    })
                                })
                        }) && let Some(duration) =
                            crate::voice::phrase(&phrases, &stream.locale(), "robot.do_it_for_me")
                                .and_then(|wav| wav_duration(&wav))
                        {
                            let ready =
                                confirmation_ready_at(Instant::now(), audio_until, duration);
                            offered_at = Some(ready);
                            audio_until = ready - Duration::from_millis(300);
                            prompt_epoch = speech_epoch.load(Ordering::Acquire);
                            say_line(&stream, &phrases, &locale, generation, "robot.do_it_for_me");
                            confirmation_offer = Some(offer);
                        }
                        if confirmation_offer.is_none() {
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
                        // The offer owns the pre-prompt snapshot; a yes cannot create a new one.
                        let confirmation = confirmed_click.take();
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
                            mm_win32::input::Win32Injector
                                .execute_verified(target, &action, confirmation)
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
                            if let Some(wav) = crate::voice::phrase(
                                &phrases,
                                &stream.locale(),
                                "robot.thinking_harder",
                            ) {
                                if let Some(duration) = wav_duration(&wav) {
                                    audio_until = audio_until.max(Instant::now()) + duration;
                                } else {
                                    audio_known = false;
                                    offered_at = None;
                                }
                            }
                        }
                    }
                    TeacherCommand::RequestScreenshot => {
                        offered_at = None;
                        let Some((response_id, Some(call_id))) = last_turn.clone() else {
                            finish(&app, &stream, generation);
                            return;
                        };
                        let mut asked = false;
                        let waiting = Instant::now();
                        let next_target = loop {
                            if cancelled() {
                                return;
                            }
                            if screenshot_wait_expired(
                                waiting.elapsed(),
                                progress.elapsed(),
                                started.elapsed(),
                            ) {
                                say_line(&stream, &phrases, &locale, generation, "robot.sleep");
                                finish(&app, &stream, generation);
                                return;
                            }
                            if let Some(front) = mm_win32::window::foreground_target(&[])
                                && (front.window == current.snapshot.target
                                    || mm_win32::window::lesson_followup(
                                        lesson_window,
                                        front.window,
                                        existing_windows.as_deref(),
                                    ))
                            {
                                break front.window;
                            }
                            if !asked {
                                say_line(
                                    &stream,
                                    &phrases,
                                    &locale,
                                    generation,
                                    "robot.click_app_first",
                                );
                                asked = true;
                            }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        };
                        match look(&app, generation, own.clone(), Some(next_target)).await {
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
                    let live_offer = offered_at.is_some() && confirmation_offer.is_some();
                    let input = if let Some(ready) = offered_at.filter(|_| live_offer) {
                        confirmation_answer(&words.text, words.pressed_at, ready,
                            prompt_epoch, speech_epoch.load(Ordering::Acquire), &locale).map(|yes| TeacherInput::DoItAnswer {
                            yes,
                            now_ms: now(),
                        })
                    } else {
                        None
                    }.or_else(|| (!live_offer && asks_do_it(&words.text)).then(|| {
                        TeacherInput::DoItRequested { now_ms: now() }
                    }));
                    let taken = live_offer || input.is_some();
                    // A superseded recognition task cannot authorize a click.
                    if words.taken.send(taken).is_err() {
                        continue;
                    }
                    if let Some(input) = input {
                        if let TeacherInput::DoItAnswer { yes, .. } = &input {
                            confirmed_click = confirmation_offer
                                .take()
                                .and_then(|offer| offer.answer(*yes));
                            offered_at = None;
                        }
                        pending = teacher.input(input);
                    } else if live_offer {
                        // PTT may have discarded queued audio. Keep the old deadline as a
                        // conservative bound and require a fresh press after the repeat.
                        if let Some(duration) = crate::voice::phrase(&phrases, &stream.locale(), "robot.do_it_for_me")
                            .and_then(|wav| wav_duration(&wav)) {
                            offered_at = Some(confirmation_ready_at(Instant::now(), audio_until, duration));
                            audio_until = offered_at.unwrap() - Duration::from_millis(300);
                            prompt_epoch = speech_epoch.load(Ordering::Acquire);
                            say_line(&stream, &phrases, &locale, generation, "robot.do_it_for_me");
                        } else {
                            offered_at = None;
                            confirmation_offer = None;
                            let _ = teacher.input(TeacherInput::DoItAnswer { yes: false, now_ms: now() });
                        }
                    }
                }
                input = received.recv() => {
                    let Some(input) = input else {
                        // The click thread is gone: no step can complete any more.
                        finish(&app, &stream, generation);
                        return;
                    };
                    let (stale, focus_only) =
                        input_staleness(&input, pointed_ms, current.snapshot.target);
                    if focus_only && !asked_to_focus && teacher.matches_key_chord(&input) {
                        asked_to_focus = true;
                        offered_at = None;
                        confirmation_offer = None;
                        let _ = teacher.input(TeacherInput::DoItAnswer { yes: false, now_ms: now() });
                        say_line(&stream, &phrases, &locale, generation, "robot.click_app_first");
                        if let Some(wav) = crate::voice::phrase(&phrases, &stream.locale(), "robot.click_app_first") {
                            if let Some(duration) = wav_duration(&wav) {
                                audio_until = audio_until.max(Instant::now()) + duration;
                            } else {
                                audio_known = false;
                            }
                        }
                    }
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

pub(super) fn screenshot_wait_expired(waiting: Duration, idle: Duration, lesson: Duration) -> bool {
    waiting >= OFFER_LIMIT || idle >= IDLE_LIMIT || lesson >= LESSON_LIMIT
}

/// Whether input is stale, and whether window focus is its only stale condition.
pub(super) fn input_staleness(
    input: &TeacherInput,
    pointed_ms: u64,
    target: mm_platform::TargetWindow,
) -> (bool, bool) {
    match input {
        TeacherInput::KeyChord {
            now_ms, foreground, ..
        } => {
            // A shared process (browsers and UWP hosts) does not identify the lesson.
            let in_front = foreground.is_none_or(|front| {
                front.process_id == target.process_id && front.handle == target.handle
            });
            (
                *now_ms < pointed_ms || !in_front,
                *now_ms >= pointed_ms && !in_front,
            )
        }
        TeacherInput::Click { now_ms, .. } | TeacherInput::DoubleClick { now_ms, .. } => {
            (*now_ms < pointed_ms, false)
        }
        _ => (false, false),
    }
}

fn expire_offer(teacher: &mut Teacher, offered_at: &mut Option<Instant>, now_ms: u64) {
    if offered_at.is_some_and(|at| Instant::now().saturating_duration_since(at) >= OFFER_LIMIT) {
        *offered_at = None;
        // Reset Awaiting without speaking. A later explicit request can offer again.
        let _ = teacher.input(TeacherInput::DoItUnavailable { now_ms });
    }
}

#[cfg(test)]
pub(super) fn fresh_answer(pressed_at: Option<Instant>, offered_at: Option<Instant>) -> bool {
    matches!((pressed_at, offered_at), (Some(pressed), Some(offered)) if pressed > offered)
}
