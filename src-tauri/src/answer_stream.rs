//! Rust → Mate's WebView: one ordered channel for the robot's state, the answer's timeline
//! items, its speech PCM, barge-in cancels and localized lines (src/ipc/answer-stream.ts
//! is the other end). One channel keeps items and audio in order.

use std::sync::Mutex;
use std::time::Instant;

use base64::Engine;
use mm_core::matescript::Item;
use serde::Serialize;
use tauri::ipc::Channel;

#[derive(Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AnswerMsg {
    /// "listening", "thinking" or "idle".
    State {
        generation: u64,
        state: &'static str,
    },
    Item {
        generation: u64,
        item: Item,
    },
    /// PCM16LE mono, base64. Sentence 0 is a cached phrase outside the timeline; an empty
    /// chunk with `eos` ends a sentence that has no audio.
    Pcm {
        generation: u64,
        sentence: u64,
        sample_rate: u32,
        eos: bool,
        samples_b64: String,
    },
    /// Barge-in: everything of generations up to this one stops.
    Cancel {
        generation: u64,
    },
    /// A locale key the WebView shows in the bubble.
    Line {
        generation: u64,
        key: &'static str,
    },
    /// The person's current transcript, capped at 200 characters; never log its text.
    Heard {
        generation: u64,
        text: String,
    },
    /// A closed display id, never the proxy's raw model header.
    Model {
        generation: u64,
        model: Option<&'static str>,
    },
    /// The microphone's RMS level (0..=1) while listening, ~12 Hz: Mate's ears move.
    Level {
        generation: u64,
        level: f32,
    },
}

/// The subscribed channel and Windows locale the WebView reported, and what it reported
/// about playback.
#[derive(Default)]
pub struct AnswerStream {
    inner: Mutex<Option<(Channel<AnswerMsg>, String)>>,
    /// The newest cancelled generation: a dying task's late message for it, or an older
    /// one, never reaches the window after its `Cancel`.
    cancelled: Mutex<Option<u64>>,
    reports: Mutex<Vec<(&'static str, u64, Instant, String)>>,
    #[cfg(debug_assertions)]
    debug: Mutex<Option<tauri::AppHandle>>,
}

/// The playback reports the window may send (src/ipc/answer-stream.ts `reportPlayback`).
const REPORTS: [&str; 7] = [
    "audio_running",
    "audio_suspended",
    "done",
    "pcm",
    "dropped",
    "failed",
    "trace",
];
const KEPT_REPORTS: usize = 64;

/// Normalize the served model to a closed display id; absent/unknown headers stay hidden.
pub fn model_chip(served: Option<&str>) -> Option<&'static str> {
    match served {
        Some(model) if model.starts_with("muse-spark") => Some("muse-spark-1.3"),
        Some(model) if model.starts_with("muse-wonder") => Some("muse-wonder-1.0"),
        _ => None,
    }
}

#[cfg(test)]
mod model_tests {
    use super::{AnswerMsg, model_chip};

    #[test]
    fn model_message_uses_the_answer_wire_format() {
        let message = AnswerMsg::Model {
            generation: 7,
            model: Some("muse-spark-1.3"),
        };
        assert_eq!(
            serde_json::to_value(message).unwrap(),
            serde_json::json!({"type": "model", "generation": 7, "model": "muse-spark-1.3"})
        );
    }

    #[test]
    fn unknown_or_missing_model_clears_the_chip() {
        for served in [None, Some("unknown-model")] {
            let message = AnswerMsg::Model {
                generation: 7,
                model: model_chip(served),
            };
            assert_eq!(
                serde_json::to_value(message).unwrap(),
                serde_json::json!({"type": "model", "generation": 7, "model": null})
            );
        }
    }

    #[test]
    fn only_known_served_prefixes_become_chips() {
        for (served, expected) in [
            (Some("muse-spark-1.3"), Some("muse-spark-1.3")),
            (Some("muse-spark-preview"), Some("muse-spark-1.3")),
            (Some("muse-wonder-1.0"), Some("muse-wonder-1.0")),
            (Some("muse-wonder-preview"), Some("muse-wonder-1.0")),
            (None, None),
            (Some(""), None),
            (Some("other-muse-spark"), None),
            (Some("sam-3.1"), None),
            (Some("muse-voice-transcribe-1.0"), None),
        ] {
            assert_eq!(model_chip(served), expected);
        }
    }
}

impl AnswerStream {
    /// Whether `generation` was cancelled (a newer press): its work must not touch the
    /// screen any more.
    pub fn is_cancelled(&self, generation: u64) -> bool {
        self.cancelled
            .lock()
            .is_ok_and(|cancelled| cancelled.is_some_and(|through| generation <= through))
    }

    pub fn subscribed(&self) -> bool {
        self.inner.lock().is_ok_and(|inner| inner.is_some())
    }

    /// Every report so far: event, generation, detail.
    pub fn reports(&self) -> Vec<(&'static str, u64, String)> {
        self.reports.lock().map_or_else(
            |_| Vec::new(),
            |reports| {
                reports
                    .iter()
                    .map(|(event, generation, _, detail)| (*event, *generation, detail.clone()))
                    .collect()
            },
        )
    }

    /// The latest report of `event` for `generation`, if any.
    pub fn reported(&self, event: &str, generation: u64) -> Option<Instant> {
        let reports = self.reports.lock().ok()?;
        reports
            .iter()
            .rev()
            .find(|(name, number, ..)| *name == event && *number == generation)
            .map(|(_, _, at, _)| *at)
    }

    /// Connects the development debug mirror (`debug_view.rs`) once at startup.
    #[cfg(debug_assertions)]
    pub fn attach_debug(&self, app: tauri::AppHandle) {
        if let Ok(mut debug) = self.debug.lock() {
            *debug = Some(app);
        }
    }

    /// Sends to the WebView; a message before it subscribed, or to a closed window, is dropped.
    pub fn send(&self, message: AnswerMsg) {
        // Held while sending, so a Cancel and a late message never cross.
        let Ok(mut cancelled) = self.cancelled.lock() else {
            return;
        };
        match &message {
            AnswerMsg::Cancel { generation } => {
                *cancelled = Some(cancelled.map_or(*generation, |old| old.max(*generation)));
            }
            AnswerMsg::State { generation, .. }
            | AnswerMsg::Item { generation, .. }
            | AnswerMsg::Pcm { generation, .. }
            | AnswerMsg::Line { generation, .. }
            | AnswerMsg::Heard { generation, .. }
            | AnswerMsg::Model { generation, .. }
            | AnswerMsg::Level { generation, .. } => {
                if cancelled.is_some_and(|through| *generation <= through) {
                    return;
                }
            }
        }
        #[cfg(debug_assertions)]
        if let Some(app) = self.debug.lock().ok().and_then(|debug| debug.clone()) {
            crate::debug_view::answer(&app, &message);
        }
        let channel = self
            .inner
            .lock()
            .ok()
            .and_then(|inner| inner.as_ref().map(|(channel, _)| channel.clone()));
        if let Some(channel) = channel {
            let _ = channel.send(message);
        }
    }

    /// The chosen UI locale, falling back to the WebView's Windows language.
    pub fn locale(&self) -> String {
        let system = self
            .inner
            .lock()
            .ok()
            .and_then(|inner| inner.as_ref().map(|(_, locale)| locale.clone()))
            .unwrap_or_else(|| "en".into());
        crate::settings::language().resolve(&system).to_owned()
    }

    /// One sentence's WAV as a single PCM chunk; anything but mono PCM16 becomes an empty
    /// chunk, so the sentence still ends.
    pub fn send_wav(&self, generation: u64, sentence: u64, wav: &[u8]) {
        let (sample_rate, samples) = match mm_core::wav::pcm16_samples(wav) {
            // The window takes 8-48 kHz only (src/ipc/answer-stream.ts).
            Ok((rate @ 8_000..=48_000, 1, samples)) => (rate, samples),
            _ => (16_000, &[][..]),
        };
        self.send(AnswerMsg::Pcm {
            generation,
            sentence,
            sample_rate,
            eos: true,
            samples_b64: base64::engine::general_purpose::STANDARD.encode(samples),
        });
    }

    pub fn end_sentence(&self, generation: u64, sentence: u64) {
        self.send_wav(generation, sentence, &[]);
    }
}

/// The window's playback reports: the audio engine's state, an answer played out.
#[tauri::command]
pub fn report_playback(
    event: String,
    generation: u64,
    detail: Option<String>,
    stream: tauri::State<'_, AnswerStream>,
) {
    let Some(event) = REPORTS.into_iter().find(|known| *known == event) else {
        return;
    };
    if let Ok(mut reports) = stream.reports.lock() {
        if reports.len() >= KEPT_REPORTS {
            reports.remove(0);
        }
        // A short diagnostic (the audio clock): plain characters only, never answer text.
        let detail: String = detail
            .unwrap_or_default()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || " .=_-".contains(*c))
            .take(80)
            .collect();
        reports.push((event, generation, Instant::now(), detail));
    }
}

/// Mate reports the Windows language separately from the saved preference.
#[tauri::command]
pub fn subscribe_answer_stream(
    app: tauri::AppHandle,
    channel: Channel<AnswerMsg>,
    locale: String,
    stream: tauri::State<'_, AnswerStream>,
) {
    let locale = if locale == "ru" { "ru" } else { "en" };
    if let Ok(mut inner) = stream.inner.lock() {
        *inner = Some((channel, locale.into()));
    }
    crate::refresh_tray_labels(&app);
}
