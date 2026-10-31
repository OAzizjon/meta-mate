use super::*;

/// Check the whole snapshot ancestry, including the window's title, before offering.
pub(super) fn pressable(snapshot: &mm_platform::UiSnapshot, rect: PhysRect) -> bool {
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
    let child_type = element.control_type.clone();
    let mut named = false;
    for level in 0..snapshot.elements.len() {
        if hidden.contains(&element.id)
            || mm_core::privacy::forbidden(&element.name, &element.control_type)
        {
            return false;
        }
        if level == 0
            || (level == 1
                && mm_platform::can_borrow_click_name(&child_type, &element.control_type))
        {
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
pub(super) const PRAISE: [&str; 4] = [
    "robot.right_click",
    "robot.right_click_2",
    "robot.right_click_3",
    "robot.right_click_4",
];

pub(super) fn line_key(line: LineKey, steps_done: &mut usize) -> Option<&'static str> {
    match line {
        LineKey::WrongPlace => Some("robot.wrong_place"),
        LineKey::WrongKeys => Some("robot.wrong_keys"),
        LineKey::DoubleClick => Some("robot.double_click"),
        LineKey::ScreenChanged => Some("robot.screen_changed"),
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
pub(super) fn targets_for(look: &Look, response: &ParsedResponse) -> Vec<Option<PhysRect>> {
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
pub(super) fn lesson_session(
    model: ModelChoice,
    (sent_width, sent_height): (u32, u32),
) -> ComputerSession {
    ComputerSession {
        model,
        effort: ReasoningEffort::Minimal,
        sent_width,
        sent_height,
    }
}

pub(super) fn fell_back(asked: ModelChoice, served: Option<&str>) -> bool {
    asked == ModelChoice::Thorough && served.is_some_and(|model| model.starts_with("muse-spark"))
}

/// Resolve unlisted clicks live after a cut walk, then try SAM's outline when allowed.
/// Otherwise the step stays unplaceable and the teacher refuses (invariant 5).
pub(super) async fn outline_unlisted(
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
pub(super) fn wav_duration(bytes: &[u8]) -> Option<Duration> {
    let wav = mm_core::wav::read(bytes).ok()?;
    Some(Duration::from_secs_f64(
        wav.samples.len() as f64 / f64::from(wav.sample_rate) / f64::from(wav.channels),
    ))
}

pub(super) async fn speak(
    stream: &AnswerStream,
    proxy: &Proxy,
    _locale: &str,
    generation: u64,
    id: u64,
    text: String,
    speech_epoch: &AtomicU64,
) -> Option<Duration> {
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
    match proxy.speak(voice_of(&stream.locale()), &text).await {
        Ok(speech) if speech_epoch.load(Ordering::Acquire) == epoch => {
            stream.send_wav(generation, id, &speech.wav);
            wav_duration(&speech.wav)
        }
        Ok(_) => {
            stream.end_sentence(generation, id);
            Some(Duration::ZERO)
        }
        Err(error) => {
            eprintln!("teacher: speech synthesis: {error}");
            stream.end_sentence(generation, id);
            Some(Duration::ZERO)
        }
    }
}

/// The lesson's timeline ends and the beam goes out (unless a newer press took over).
pub(super) fn finish(app: &AppHandle, stream: &AnswerStream, generation: u64) {
    if stream.is_cancelled(generation) {
        return;
    }
    stream.send(AnswerMsg::Item {
        generation,
        item: Item::End,
    });
    stop_pointing(app);
}
