/// Sound policy selected by the user, independently of the robot's mood.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Gentle sounds only, at most two per answer.
    #[default]
    Calm,
    /// All sounds, at most four per answer.
    Fun,
}

/// Validation context for one answer. Coordinates use screenshot pixels.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Language used when the answer omits a language tag.
    pub language: String,
    /// Sound policy for this answer.
    pub mode: Mode,
    /// Element IDs offered to the model for this screenshot.
    pub point_ids: Vec<String>,
    /// Screenshot width and height; absent dimensions reject coordinate points.
    pub screenshot: Option<(u32, u32)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "en".into(),
            mode: Mode::Calm,
            point_ids: Vec::new(),
            screenshot: None,
        }
    }
}
