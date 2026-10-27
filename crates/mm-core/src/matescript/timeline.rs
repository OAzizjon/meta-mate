use std::fmt;

use serde::{Deserialize, Serialize};

/// Playback volume for a sound or ambience bed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Quiet playback.
    Soft,
    /// Regular playback.
    Normal,
    /// Emphatic playback, available in fun mode only.
    Loud,
}

/// A validated sound effect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Sound {
    /// Canonical sound library name.
    pub name: String,
    /// Validated playback volume.
    pub level: Level,
    /// Whether speech must wait for the sound to finish.
    pub solo: bool,
}

/// A validated target in the current screenshot. Deserialized when the WebView hands it
/// back to be shown (`point_at`): Rust resolves it again against its own look.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Point {
    /// An element from the supplied candidate list.
    Id(String),
    /// A pixel inside the screenshot.
    Pixel(u32, u32),
}

/// One app-generated instruction in a guide.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Step {
    /// One-based step number.
    pub number: u32,
    /// Total number of steps.
    pub total: u32,
    /// Validated target for this instruction.
    pub target: Point,
}

/// An action that starts with the next sentence, or plays alone at answer end.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Bound {
    /// Play a sound alongside speech.
    Sfx(Sound),
    /// Change the robot's mood.
    Mood(String),
    /// Highlight a target.
    Point(Point),
    /// Present a guide instruction.
    Step(Step),
    /// Present a short text bubble.
    Show(String),
}

impl fmt::Debug for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sfx(sound) => f.debug_tuple("Sfx").field(sound).finish(),
            Self::Mood(mood) => f.debug_tuple("Mood").field(mood).finish(),
            Self::Point(point) => f.debug_tuple("Point").field(point).finish(),
            Self::Step(step) => f.debug_tuple("Step").field(step).finish(),
            Self::Show(text) => f
                .debug_struct("Show")
                .field("chars", &text.chars().count())
                .finish(),
        }
    }
}

/// One sentence and the actions that start with it.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct Say {
    /// Sequential sentence ID, starting at one.
    pub id: u64,
    /// Whitespace-normalized speech text.
    pub text: String,
    /// Actions in source order, with duplicate singleton kinds replaced.
    pub bound: Vec<Bound>,
}

impl fmt::Debug for Say {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Say")
            .field("id", &self.id)
            .field("text_chars", &self.text.chars().count())
            .field("bound", &self.bound)
            .finish()
    }
}

/// A single ordered playback timeline, including optional debug diagnostics.
/// Serialized for the WebView's timeline player as `{"kind": ..., "value": ...}`
/// (`crates/mm-core/tests/snapshots/timeline_wire__wire_format.snap` is the reference).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Item {
    /// Select the TTS language.
    Lang(String),
    /// Speak a sentence and start its bound actions.
    Say(Say),
    /// Play an action with no following sentence.
    Bound(Bound),
    /// Play a solo sound as a playback barrier.
    Sfx(Sound),
    /// Start an ambience bed, or stop it when the name is absent.
    Amb { name: Option<String>, level: Level },
    /// Wait for the given number of milliseconds.
    Pause(u32),
    /// Wait for a click on the current target or a spoken next command.
    Wait,
    /// Clear the highlight and speak the localized "can't find" refusal here.
    PointRejected,
    /// Finish playback of the answer.
    End,
    /// A rejected tag; debug metadata only, never sent to speech.
    BadTag,
}
