#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModelChoice {
    /// Muse Spark.
    #[default]
    Quick,
    /// Muse Wonder.
    Thorough,
}
