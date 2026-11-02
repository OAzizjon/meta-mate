//! Rust → Mate's WebView: one ordered channel for the robot's state, the answer's timeline
//! items, its speech PCM, barge-in cancels and localized lines (src/ipc/answer-stream.ts
//! is the other end). One channel keeps items and audio in order.

use std::sync::{Mutex, PoisonError};
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
        /// Parsed timeline data only: raw model deltas must never reach the bubble.
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
    Cancel { generation: u64 },
    /// A locale key the WebView shows in the bubble.
    Line { generation: u64, key: &'static str },
    /// The person's current transcript, capped at 200 characters; never log its text.
    Heard { generation: u64, text: String },
    /// A closed display id, never the proxy's raw model header.
    Model {
        generation: u64,
        model: Option<&'static str>,
    },
    /// The microphone's RMS level (0..=1) while listening, ~12 Hz: Mate's ears move.
    Level { generation: u64, level: f32 },
}

/// The subscribed channel and Windows locale the WebView reported, and what it reported
/// about playback.
#[derive(Default)]
pub struct AnswerStream {
    /// Rust-owned target, consumed once by playback; the WebView never supplies a URL.
    opening: Mutex<Option<(u64, Option<mm_core::matescript::OpenTarget>)>>,
    inner: Mutex<Option<(Channel<AnswerMsg>, String)>>,
    chat: Mutex<Option<tauri::AppHandle>>,
    /// The newest cancelled generation: a dying task's late message for it, or an older
    /// one, never reaches the window after its `Cancel`.
    cancelled: Mutex<Option<u64>>,
    reports: Mutex<Vec<(&'static str, u64, Instant, String)>>,
    /// The "Under the hood" mirror (debug_view.rs): debug builds and developer mode.
    debug: Mutex<Option<tauri::AppHandle>>,
    /// The voice bar at the bottom of Mate's monitor (highlight.rs), and the generation
    /// of the recording it shows, if any.
    bar: Mutex<Option<(tauri::AppHandle, Option<u64>)>>,
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
        Some(model) if model.starts_with("muse-glimmer") => Some("muse-glimmer-30b"),
        _ => None,
    }
}

#[cfg(test)]
mod model_tests {
    use super::{AnswerMsg, AnswerStream, model_chip};

    #[test]
    fn opening_is_rust_owned_consumed_once_and_cancelled_with_the_answer() {
        use mm_core::matescript::{AppId, Item, OpenTarget};
        let stream = AnswerStream::default();
        let item = AnswerMsg::Item {
            generation: 7,
            item: Item::Open(OpenTarget::App(AppId::Paint)),
        };
        stream.send(item.clone());
        assert!(stream.take_open(6).is_none());
        assert_eq!(stream.take_open(7), Some(OpenTarget::App(AppId::Paint)));
        stream.send(item);
        assert!(stream.take_open(7).is_none());
        stream.send(AnswerMsg::Item {
            generation: 8,
            item: Item::Open(OpenTarget::App(AppId::Calculator)),
        });
        stream.send(AnswerMsg::Cancel { generation: 8 });
        assert!(stream.take_open(8).is_none());
    }

    #[test]
    fn streamed_tags_never_become_bubble_or_speech_text() {
        use mm_core::matescript::{Item, Parser};

        let mut parser = Parser::default();
        let mut items = Vec::new();
        for chunk in [
            "<la",
            "ng en><mood happy><show Save>Choose Sa",
            "ve.</mood>",
        ] {
            items.extend(parser.push(chunk));
        }
        items.extend(parser.finish());
        let messages: Vec<_> = items
            .into_iter()
            .filter(|item| matches!(item, Item::Say(_)))
            .map(|item| {
                serde_json::to_value(AnswerMsg::Item {
                    generation: 7,
                    item,
                })
                .unwrap()
            })
            .collect();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["item"]["value"]["text"], "Choose Save.");
        let bound = messages[0]["item"]["value"]["bound"].as_array().unwrap();
        assert!(bound.iter().any(|tag| tag["value"] == "Save"), "{bound:?}");
    }

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
            (Some("muse-glimmer-30b"), Some("muse-glimmer-30b")),
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
    pub fn take_open(&self, generation: u64) -> Option<mm_core::matescript::OpenTarget> {
        if self.is_cancelled(generation) {
            return None;
        }
        let mut opening = self.opening.lock().ok()?;
        let (owner, target) = opening.as_mut()?;
        (*owner == generation).then(|| target.take()).flatten()
    }

    pub fn attach_chat(&self, app: tauri::AppHandle) {
        *self.chat.lock().unwrap_or_else(PoisonError::into_inner) = Some(app);
    }

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

    /// Connects the voice bar once the highlight windows exist.
    pub fn attach_voice_bar(&self, app: tauri::AppHandle) {
        if let Ok(mut bar) = self.bar.lock() {
            *bar = Some((app, None));
        }
    }

    /// Listening raises the bar for that recording; only its level and words follow, and
    /// only its own next state (or a cancel reaching it) lowers it: a lesson still talking
    /// in an older generation never touches the bar of a newer recording.
    #[cfg(windows)]
    fn voice_bar(&self, message: &AnswerMsg) {
        let Ok(mut guard) = self.bar.lock() else {
            return;
        };
        let Some((app, listening)) = guard.as_mut() else {
            return;
        };
        use crate::highlight::VoiceBarMsg;
        let next = match (message, *listening) {
            (
                AnswerMsg::State {
                    generation,
                    state: "listening",
                },
                _,
            ) => {
                *listening = Some(*generation);
                crate::highlight::follow_mate(app);
                VoiceBarMsg::State { state: "listening" }
            }
            (AnswerMsg::Level { generation, level }, Some(shown)) if *generation == shown => {
                VoiceBarMsg::Level { level: *level }
            }
            (AnswerMsg::Heard { generation, text }, Some(shown)) if *generation == shown => {
                VoiceBarMsg::Heard { text: text.clone() }
            }
            (AnswerMsg::State { generation, state }, Some(shown)) if *generation >= shown => {
                *listening = None;
                VoiceBarMsg::State {
                    state: if *state == "idle" { "idle" } else { "thinking" },
                }
            }
            (AnswerMsg::Cancel { generation }, Some(shown)) if *generation >= shown => {
                *listening = None;
                VoiceBarMsg::State { state: "idle" }
            }
            _ => return,
        };
        crate::highlight::voice_bar(app, &next);
    }

    /// Connects the "Under the hood" mirror (`debug_view.rs`) once at startup.
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
        if let AnswerMsg::Item {
            generation,
            item: Item::Open(target),
        } = &message
            && let Ok(mut opening) = self.opening.lock()
            && opening.as_ref().is_none_or(|(owner, _)| generation > owner)
        {
            *opening = Some((*generation, Some(target.clone())));
        }
        #[cfg(windows)]
        self.voice_bar(&message);
        if let Some(app) = self.debug.lock().ok().and_then(|debug| debug.clone()) {
            crate::debug_view::answer(&app, &message);
        }
        if let Some(app) = self.chat.lock().ok().and_then(|chat| chat.clone()) {
            crate::chat::record(&app, &message, &self.locale());
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
            .unwrap_or_else(|| {
                #[cfg(windows)]
                {
                    mm_win32::locale::system_locale().into()
                }
                #[cfg(not(windows))]
                {
                    "en".into()
                }
            });
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
