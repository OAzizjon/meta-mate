use crate::api::response::{Action, ParsedResponse};
use crate::geom::PhysRect;

use super::confirmation::Confirmation;
use super::step::{Step, immediate, same_chord, supported};
use super::{LineKey, ModelChoice, RefusalReason, TeacherCommand, TeacherConfig, TeacherInput};

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
        }
    }

    /// Consume one event. Backward timestamps are clamped to the latest time.
    /// Terminal states ignore all later events.
    pub fn input(&mut self, input: TeacherInput) -> Vec<TeacherCommand> {
        let mut out = Vec::new();
        if self.terminal {
            return out;
        }
        let timestamp = match &input {
            TeacherInput::Click { now_ms, .. }
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
            TeacherInput::ModelTurn {
                response,
                frame: _,
                targets,
            } => {
                self.model_turn(response, targets, &mut out);
            }
            TeacherInput::WonderUnavailable => {
                self.wonder_unavailable = true;
                self.model = ModelChoice::Quick;
                out.push(TeacherCommand::UseModel(ModelChoice::Quick));
            }
            TeacherInput::ModelFailed { .. } => {
                self.model_failures = self.model_failures.saturating_add(1);
                if self.model_failures >= 2 {
                    self.refuse(
                        RefusalReason::ModelUnavailable,
                        LineKey::NetworkError,
                        &mut out,
                    );
                }
            }
            TeacherInput::ScreenChangedUnexpectedly { .. } => {
                self.steps.clear();
                self.confirmations.clear();
                self.settled_from = None;
                self.failures = 0;
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
            out.push(TeacherCommand::RequestScreenshot);
        }
        out
    }

    fn model_turn(
        &mut self,
        response: ParsedResponse,
        targets: Vec<Option<PhysRect>>,
        out: &mut Vec<TeacherCommand>,
    ) {
        self.turns += 1;
        self.model_failures = 0;
        self.settled_from = None;
        out.extend(
            response
                .commentary
                .into_iter()
                .map(TeacherCommand::SpeakCommentary),
        );
        if self.turns > u16::from(self.config.max_turns) {
            self.refuse(RefusalReason::TooManyTurns, LineKey::TooManyTurns, out);
            return;
        }
        if response.computer_calls.is_empty() {
            self.terminal = true;
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
                if matches!(action, Action::Unknown { .. })
                    || rect.is_none_or(|rect| rect.left >= rect.right || rect.top >= rect.bottom)
                {
                    self.refuse(RefusalReason::TargetNotFound, LineKey::CantFind, out);
                    return;
                }
                if !supported(&action) {
                    self.refuse(RefusalReason::UnsupportedAction, LineKey::WontPress, out);
                    return;
                }
                if let Some(rect) = rect {
                    self.steps.push(Step {
                        action,
                        rect,
                        call: call_index,
                    });
                }
            }
        }
        if self.steps.len() > usize::from(u8::MAX) {
            self.refuse(RefusalReason::TooManyTurns, LineKey::TooManyTurns, out);
            return;
        }
        self.present(out);
    }

    fn human_event(&mut self, event: TeacherInput, out: &mut Vec<TeacherCommand>) {
        let Some(step) = self.steps.get(self.index) else {
            return;
        };
        let is_click = matches!(
            step.action,
            Action::Click { .. } | Action::DoubleClick { .. }
        );
        let (completed, human) = match (&step.action, &event) {
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
