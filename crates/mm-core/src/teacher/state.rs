use crate::api::response::{Action, ParsedResponse};
use crate::geom::{FrameTransform, PhysRect, ShotPoint};

use super::confirmation::Confirmation;
use super::step::{Step, immediate, same_chord, supported};
use super::{LineKey, ModelChoice, RefusalReason, TeacherCommand, TeacherConfig, TeacherInput};

#[derive(Default, PartialEq, Eq)]
enum DoItState {
    #[default]
    Pointing,
    Awaiting,
    Pressing,
}

pub struct Teacher {
    config: TeacherConfig,
    steps: Vec<Step>,
    confirmations: Vec<Confirmation>,
    index: usize,
    turns: u16,
    failures: u8,
    model_failures: u8,
    model: ModelChoice,
    wonder_unavailable: bool,
    now_ms: u64,
    settled_from: Option<u64>,
    terminal: bool,
    do_it: DoItState,
    offered: bool,
}

impl Teacher {
    pub fn new(config: TeacherConfig) -> Self {
        Self {
            model: config.configured_model,
            config,
            steps: Vec::new(),
            confirmations: Vec::new(),
            index: 0,
            turns: 0,
            failures: 0,
            model_failures: 0,
            wonder_unavailable: false,
            now_ms: 0,
            settled_from: None,
            terminal: false,
            do_it: DoItState::Pointing,
            offered: false,
        }
    }

    /// Whether a human chord matches the active step, without advancing it.
    pub fn matches_key_chord(&self, input: &TeacherInput) -> bool {
        !self.terminal
            && self.settled_from.is_none()
            && self.do_it != DoItState::Pressing
            && matches!(
                (self.steps.get(self.index).map(|step| &step.action), input),
                (Some(Action::Keypress { keys }), TeacherInput::KeyChord {
                    keys: actual, injected: false, ..
                }) if same_chord(keys, actual)
            )
    }

    /// Consume one event. Backward timestamps are clamped to the latest time.
    /// Terminal states ignore all later events.
    pub fn input(&mut self, input: TeacherInput) -> Vec<TeacherCommand> {
        let mut out = Vec::new();
        if self.terminal {
            return out;
        }
        let timestamp = match &input {
            TeacherInput::DoItRequested { now_ms }
            | TeacherInput::DoItAnswer { now_ms, .. }
            | TeacherInput::DoItUnavailable { now_ms }
            | TeacherInput::DoItDone { now_ms, .. }
            | TeacherInput::Click { now_ms, .. }
            | TeacherInput::DoubleClick { now_ms, .. }
            | TeacherInput::KeyChord { now_ms, .. }
            | TeacherInput::ValueChanged { now_ms, .. }
            | TeacherInput::UserSaidNext { now_ms }
            | TeacherInput::ScreenChangedUnexpectedly { now_ms }
            | TeacherInput::Tick { now_ms }
            | TeacherInput::ModelFailed { now_ms } => Some(*now_ms),
            _ => None,
        };
        if let Some(now_ms) = timestamp {
            self.now_ms = self.now_ms.max(now_ms);
        }
        match input {
            TeacherInput::DoItRequested { .. } => {
                if self.steps.get(self.index).is_some() && !self.pressable() {
                    out.push(TeacherCommand::Say(LineKey::PressFailed));
                } else {
                    self.offer_do_it(&mut out);
                }
            }
            TeacherInput::DoItAnswer { yes, .. } => {
                if self.do_it == DoItState::Awaiting {
                    if yes {
                        if let Some(step) = self.steps.get(self.index) {
                            self.do_it = DoItState::Pressing;
                            out.push(TeacherCommand::Say(LineKey::Pressing));
                            out.push(TeacherCommand::PerformClick {
                                at: step.rect.center(),
                                rect: step.rect,
                            });
                        }
                    } else {
                        self.do_it = DoItState::Pointing;
                        out.push(TeacherCommand::Say(LineKey::PressCancelled));
                    }
                }
            }
            TeacherInput::DoItUnavailable { .. } => {
                self.do_it = DoItState::Pointing;
                out.push(TeacherCommand::Say(LineKey::WontPress));
            }
            TeacherInput::DoItDone { ok, .. } => {
                if self.do_it == DoItState::Pressing {
                    self.do_it = DoItState::Pointing;
                    if ok {
                        // The explicit human yes authorizes this one driver result. No
                        // praise: Mate pressed it, not the person.
                        self.complete(true, &mut out);
                        self.present(&mut out);
                    } else {
                        out.push(TeacherCommand::Say(LineKey::PressFailed));
                    }
                }
            }
            TeacherInput::ModelTurn {
                response,
                frame,
                targets,
            } => {
                self.model_turn(response, frame, targets, &mut out);
            }
            TeacherInput::WonderUnavailable => {
                self.wonder_unavailable = true;
                self.model = ModelChoice::Quick;
                out.push(TeacherCommand::UseModel(ModelChoice::Quick));
            }
            TeacherInput::ModelFailed { .. } => self.model_failed(&mut out),
            TeacherInput::ScreenChangedUnexpectedly { .. } => {
                self.steps.clear();
                self.confirmations.clear();
                self.settled_from = None;
                self.failures = 0;
                self.do_it = DoItState::Pointing;
                self.offered = false;
                self.restore_quick(&mut out);
                out.push(TeacherCommand::RequestScreenshot);
            }
            event => self.human_event(event, &mut out),
        }
        if self
            .settled_from
            .is_some_and(|start| self.now_ms.saturating_sub(start) >= self.config.settle_ms)
        {
            self.settled_from = None;
            if self.turns >= u16::from(self.config.max_turns) {
                self.refuse(RefusalReason::TooManyTurns, LineKey::TooManyTurns, &mut out);
            } else {
                out.push(TeacherCommand::RequestScreenshot);
            }
        }
        out
    }

    fn model_turn(
        &mut self,
        response: ParsedResponse,
        frame: FrameTransform,
        targets: Vec<Option<PhysRect>>,
        out: &mut Vec<TeacherCommand>,
    ) {
        self.turns += 1;
        self.do_it = DoItState::Pointing;
        self.offered = false;
        self.settled_from = None;
        if self.turns > u16::from(self.config.max_turns) {
            self.refuse(RefusalReason::TooManyTurns, LineKey::TooManyTurns, out);
            return;
        }
        // A turn cut off by the token cap (or still reasoning) has no action yet: that
        // is a failed call, never "done".
        if response.status != "completed" || response.incomplete_reason.is_some() {
            self.model_failed(out);
            return;
        }
        self.model_failures = 0;
        // The spoken line is played only once the turn is known to be usable: on a
        // refused turn Mate must not first say "click here" (invariant 5).
        let commentary: Vec<TeacherCommand> = response
            .commentary
            .into_iter()
            .map(TeacherCommand::SpeakCommentary)
            .collect();
        if response.computer_calls.is_empty() {
            self.terminal = true;
            out.extend(commentary);
            out.extend([TeacherCommand::Finished, TeacherCommand::Say(LineKey::Done)]);
            return;
        }
        self.steps.clear();
        self.confirmations.clear();
        self.index = 0;
        self.failures = 0;
        // Validate the whole batch before pointing or accepting any confirmation.
        let mut targets = targets.into_iter();
        for call in response.computer_calls {
            let call_index = self.confirmations.len();
            self.confirmations.push(Confirmation {
                ids: call
                    .safety_checks
                    .into_iter()
                    .map(|check| check.id)
                    .collect(),
                remaining: call
                    .actions
                    .iter()
                    .filter(|action| !immediate(action))
                    .count(),
                human_only: true,
            });
            for action in call.actions {
                let rect = targets.next().flatten();
                if matches!(action, Action::Unknown { .. }) {
                    self.refuse(RefusalReason::TargetNotFound, LineKey::CantFind, out);
                    return;
                }
                if !supported(&action, self.config.keyboard_steps) {
                    self.refuse(RefusalReason::UnsupportedAction, LineKey::WontPress, out);
                    return;
                }
                if rect.is_none_or(|rect| !self.plausible_target(&action, rect, frame)) {
                    self.refuse(RefusalReason::TargetNotFound, LineKey::CantFind, out);
                    return;
                }
                if let Some(rect) = rect {
                    let drag_end = match &action {
                        Action::Drag { path } => path.last().and_then(|&(x, y)| {
                            frame.shot_to_phys_checked(ShotPoint {
                                x: x as f64,
                                y: y as f64,
                            })
                        }),
                        _ => None,
                    };
                    self.steps.push(Step {
                        action,
                        rect,
                        drag_end,
                        call: call_index,
                    });
                }
            }
        }
        if self.steps.len() > usize::from(u8::MAX) {
            self.refuse(RefusalReason::TooManyTurns, LineKey::TooManyTurns, out);
            return;
        }
        out.extend(commentary);
        self.present(out);
    }

    /// The snapped element must be a real target for this action: non-empty, centred
    /// inside the captured region, and (for a click or drag start) the model's own
    /// point must lie on the screenshot and within the hit tolerance of the element.
    /// Otherwise the element came from somewhere else than the model's point (invariant 5).
    fn plausible_target(&self, action: &Action, rect: PhysRect, frame: FrameTransform) -> bool {
        if rect.left >= rect.right
            || rect.top >= rect.bottom
            || !frame.region().contains(rect.center())
        {
            return false;
        }
        let (x, y) = match action {
            Action::Click { x, y, .. } | Action::DoubleClick { x, y } => (*x, *y),
            Action::Drag { path } => {
                if path.len() < 2 {
                    return false;
                }
                let (x, y) = path[path.len() - 1];
                if frame
                    .shot_to_phys_checked(ShotPoint {
                        x: x as f64,
                        y: y as f64,
                    })
                    .is_none()
                {
                    return false;
                }
                path[0]
            }
            _ => return true,
        };
        let point = ShotPoint {
            x: x as f64,
            y: y as f64,
        };
        frame
            .shot_to_phys_checked(point)
            .is_some_and(|phys| rect.distance_to(phys) <= f64::from(self.config.hit_tolerance_px))
    }

    fn model_failed(&mut self, out: &mut Vec<TeacherCommand>) {
        self.model_failures = self.model_failures.saturating_add(1);
        if self.model_failures >= 2 {
            self.refuse(RefusalReason::ModelUnavailable, LineKey::NetworkError, out);
        }
    }

    fn human_event(&mut self, event: TeacherInput, out: &mut Vec<TeacherCommand>) {
        let Some(step) = self.steps.get(self.index) else {
            return;
        };
        let is_click = matches!(
            step.action,
            Action::Click { .. } | Action::DoubleClick { .. } | Action::Drag { .. }
        );
        let (completed, human) = match (&step.action, &event) {
            // Software input (ours included) and other buttons never complete a step:
            // only the person's own left click confirms a safety check (invariant 4).
            (_, TeacherInput::Click { injected: true, .. })
            | (_, TeacherInput::DoubleClick { injected: true, .. })
            | (_, TeacherInput::KeyChord { injected: true, .. })
            | (_, TeacherInput::Click { left: false, .. })
            | (_, TeacherInput::DoubleClick { left: false, .. }) => return,
            (Action::Click { .. }, TeacherInput::Click { at, .. })
            | (Action::DoubleClick { .. }, TeacherInput::DoubleClick { at, .. }) => {
                let distance = step.rect.distance_to(*at);
                if distance > f64::from(self.config.hit_tolerance_px) {
                    out.push(TeacherCommand::Say(LineKey::WrongPlace));
                    if distance > f64::from(self.config.near_px) {
                        self.failed_attempt(out);
                    } else {
                        self.failures = 0;
                    }
                    self.point(out);
                    return;
                }
                (true, true)
            }
            (Action::Drag { .. }, TeacherInput::Click { at, .. }) => {
                // Button-up near the mapped endpoint completes a drag. Also allow
                // a short nudge inside the start rect inflated with widened math.
                let near = i64::from(self.config.near_px);
                let x = i64::from(at.x);
                let y = i64::from(at.y);
                let near_end = step.drag_end.is_some_and(|end| {
                    (f64::from(at.x) - f64::from(end.x)).hypot(f64::from(at.y) - f64::from(end.y))
                        <= f64::from(self.config.near_px)
                });
                if !near_end
                    && (x < i64::from(step.rect.left) - near
                        || x >= i64::from(step.rect.right) + near
                        || y < i64::from(step.rect.top) - near
                        || y >= i64::from(step.rect.bottom) + near)
                {
                    out.push(TeacherCommand::Say(LineKey::WrongPlace));
                    self.failed_attempt(out);
                    self.point(out);
                    return;
                }
                (true, true)
            }
            (Action::Keypress { keys }, TeacherInput::KeyChord { keys: actual, .. }) => {
                // Other keys are ordinary typing, not a failed attempt: they must not
                // push the task toward Wonder.
                (same_chord(keys, actual), true)
            }
            (Action::Type { text }, TeacherInput::ValueChanged { text: actual, .. }) => {
                (actual.contains(text), false)
            }
            (Action::Type { .. }, TeacherInput::UserSaidNext { .. }) => (true, false),
            _ => (false, false),
        };
        if completed {
            self.complete(human, out);
            if is_click {
                out.push(TeacherCommand::Say(LineKey::RightClick));
            }
            self.present(out);
        }
    }

    fn complete(&mut self, human: bool, out: &mut Vec<TeacherCommand>) {
        self.do_it = DoItState::Pointing;
        self.offered = false;
        if let Some(step) = self.steps.get(self.index) {
            let check = &mut self.confirmations[step.call];
            if !immediate(&step.action) {
                check.remaining = check.remaining.saturating_sub(1);
                check.human_only &= human;
            }
            // Call-wide checks require all non-passive steps to have been
            // verified by hooks, and are emitted only on a completing human event.
            if check.remaining == 0 && check.human_only && human && !check.ids.is_empty() {
                out.push(TeacherCommand::AcknowledgeSafetyChecks(std::mem::take(
                    &mut check.ids,
                )));
            }
        }
        self.index += 1;
        self.failures = 0;
        self.restore_quick(out);
    }

    fn present(&mut self, out: &mut Vec<TeacherCommand>) {
        while self
            .steps
            .get(self.index)
            .is_some_and(|step| immediate(&step.action))
        {
            self.complete(false, out);
        }
        if self.index == self.steps.len() {
            self.settled_from = Some(self.now_ms);
        } else {
            out.push(TeacherCommand::Say(LineKey::StepN));
            self.point(out);
        }
    }

    fn point(&self, out: &mut Vec<TeacherCommand>) {
        if let Some(step) = self.steps.get(self.index) {
            out.push(TeacherCommand::PointAt {
                rect: step.rect,
                step: (self.index + 1) as u8,
                total: self.steps.len() as u8,
            });
        }
    }

    fn failed_attempt(&mut self, out: &mut Vec<TeacherCommand>) {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= self.config.offer_after_failures.max(1) && !self.offered {
            self.offer_do_it(out);
        }
        if self.failures >= 2
            && self.config.auto_escalate
            && self.config.configured_model == ModelChoice::Quick
            && !self.wonder_unavailable
            && self.model == ModelChoice::Quick
        {
            self.model = ModelChoice::Thorough;
            out.push(TeacherCommand::UseModel(ModelChoice::Thorough));
        }
    }

    fn pressable(&self) -> bool {
        self.steps.get(self.index).is_some_and(
            |step| matches!(&step.action, Action::Click { button, .. } if button == "left"),
        )
    }

    fn offer_do_it(&mut self, out: &mut Vec<TeacherCommand>) {
        if self.config.do_it_for_me
            && self.pressable()
            && self.do_it == DoItState::Pointing
            && let Some(step) = self.steps.get(self.index)
        {
            self.do_it = DoItState::Awaiting;
            self.offered = true;
            out.push(TeacherCommand::OfferDoIt { rect: step.rect });
        }
    }

    fn restore_quick(&mut self, out: &mut Vec<TeacherCommand>) {
        if self.config.configured_model == ModelChoice::Quick && self.model == ModelChoice::Thorough
        {
            self.model = ModelChoice::Quick;
            out.push(TeacherCommand::UseModel(ModelChoice::Quick));
        }
    }

    fn refuse(&mut self, reason: RefusalReason, line: LineKey, out: &mut Vec<TeacherCommand>) {
        self.terminal = true;
        self.settled_from = None;
        out.extend([TeacherCommand::Refused(reason), TeacherCommand::Say(line)]);
    }
}
