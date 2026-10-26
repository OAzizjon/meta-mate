#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKey {
    WrongPlace,
    RightClick,
    CantFind,
    StepN,
    Done,
    NetworkError,
    TooManyTurns,
    WontPress,
}
