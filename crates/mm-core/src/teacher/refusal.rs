#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    TargetNotFound,
    TooManyTurns,
    ModelUnavailable,
    UnsupportedAction,
}
