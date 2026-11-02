//! Session chat mirrored from the answer stream; history stays in the app data folder.
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// The longest one Mate message may grow in the history.
const MATE_TEXT_MAX_BYTES: usize = 8 * 1024;

use mm_core::matescript::{Bound, Item};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, ipc::Channel};

use crate::answer_stream::AnswerMsg;

/// Tells the chat window whether a recording listens: its Talk button becomes Stop.
const TALK_EVENT: &str = "chat-talk";
const LIMIT: usize = 200;
// JSON can escape each text byte as six ASCII bytes; include metadata overhead.
const HISTORY_MAX_BYTES: usize = LIMIT * (MATE_TEXT_MAX_BYTES * 6 + 256);

fn chat_mood(raw: &str) -> Option<&str> {
    matches!(raw, "calm" | "happy" | "surprised" | "sad").then_some(raw)
}

/// Only a closed display id (`model_chip`) survives a saved history.
fn chat_model(raw: &str) -> Option<&'static str> {
    crate::answer_stream::model_chip(Some(raw)).filter(|id| *id == raw)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    role: Role,
    text: String,
    timestamp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mood: Option<String>,
    /// The Meta model that wrote a Mate answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    // Generations restart with the process; never merge a new answer into loaded history.
    #[serde(skip)]
    generation: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Role {
    Person,
    Mate,
}

#[derive(Default)]
struct History {
    messages: Vec<Message>,
    path: Option<PathBuf>,
    subscriber: Option<Channel<Vec<Message>>>,
    cancelled: Option<u64>,
    latest: Option<u64>,
    // A lesson names its model before it speaks; an answer after its first sentences.
    model: Option<(u64, Option<&'static str>)>,
    epoch: u64,
    /// The generation of the recording that listens now, if one does.
    listening: Option<u64>,
}

#[derive(Default)]
pub struct Chat(Mutex<History>);

impl History {
    fn clear(&mut self) {
        self.messages.clear();
        self.cancelled = self.latest;
        self.epoch += 1;
    }

    fn start_request(&mut self, epoch: u64, generation: u64) -> bool {
        if epoch != self.epoch {
            return false;
        }
        self.latest = Some(self.latest.map_or(generation, |old| old.max(generation)));
        true
    }

    fn push(&mut self, role: Role, text: String, generation: Option<u64>) {
        if !valid_text(role, &text) {
            return;
        }
        self.messages.push(Message {
            role,
            text,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
            mood: None,
            model: None,
            generation,
        });
        let excess = self.messages.len().saturating_sub(LIMIT);
        self.messages.drain(..excess);
    }

    /// A recording's final transcript, as the person's message before Mate's answer; a
    /// cancelled or cleared recording leaves none.
    fn spoken(&mut self, generation: u64, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() || self.cancelled.is_some_and(|through| generation <= through) {
            return false;
        }
        // A long recording is cut to a typed question's length rather than dropped.
        let text = if text.chars().count() > 1000 {
            let mut cut: String = text.chars().take(999).collect();
            cut.push('…');
            cut
        } else {
            text.to_owned()
        };
        self.push(Role::Person, text, None);
        true
    }

    /// Whether a recording listens, from the stream's states like the voice bar's; Some
    /// when that changed. An older generation's state never ends a newer recording.
    fn talk_state(&mut self, message: &AnswerMsg) -> Option<bool> {
        let before = self.listening.is_some();
        match message {
            AnswerMsg::State {
                generation,
                state: "listening",
            } => self.listening = Some(*generation),
            AnswerMsg::State { generation, .. } | AnswerMsg::Cancel { generation }
                if self
                    .listening
                    .is_some_and(|listening| *generation >= listening) =>
            {
                self.listening = None;
            }
            _ => {}
        }
        let after = self.listening.is_some();
        (before != after).then_some(after)
    }

    fn answer(&mut self, message: &AnswerMsg, locale: &str) -> bool {
        let generation = match message {
            AnswerMsg::State { generation, .. }
            | AnswerMsg::Item { generation, .. }
            | AnswerMsg::Pcm { generation, .. }
            | AnswerMsg::Cancel { generation }
            | AnswerMsg::Line { generation, .. }
            | AnswerMsg::Heard { generation, .. }
            | AnswerMsg::Model { generation, .. }
            | AnswerMsg::Level { generation, .. } => *generation,
        };
        self.latest = Some(self.latest.map_or(generation, |old| old.max(generation)));
        if let AnswerMsg::Cancel { generation } = message {
            self.cancelled = Some(
                self.cancelled
                    .map_or(*generation, |old| old.max(*generation)),
            );
            return false;
        }
        if let AnswerMsg::Model { generation, model } = message {
            if self.cancelled.is_some_and(|through| *generation <= through) {
                return false;
            }
            self.model = Some((*generation, *model));
            return match self.messages.last_mut() {
                Some(last)
                    if last.role == Role::Mate
                        && last.generation == Some(*generation)
                        && last.model.as_deref() != *model =>
                {
                    last.model = model.map(str::to_owned);
                    true
                }
                _ => false,
            };
        }
        let (generation, text, mood) = match message {
            AnswerMsg::Item {
                generation,
                item: Item::Say(say),
            } => (
                *generation,
                say.text.clone(),
                say.bound.iter().rev().find_map(|bound| match bound {
                    Bound::Mood(name) => chat_mood(name),
                    _ => None,
                }),
            ),
            AnswerMsg::Item {
                generation,
                item: Item::PointRejected,
            } => (
                *generation,
                crate::locale_text(locale, "robot.cant_find"),
                None,
            ),
            // The listening hint is no part of the conversation: the spoken question follows.
            AnswerMsg::Line {
                key: "robot.click_stop",
                ..
            } => return false,
            AnswerMsg::Line { generation, key } => {
                (*generation, crate::locale_text(locale, key), None)
            }
            _ => return false,
        };
        if text.trim().is_empty() || self.cancelled.is_some_and(|through| generation <= through) {
            return false;
        }
        if let Some(last) = self.messages.last_mut()
            && last.role == Role::Mate
            && last.generation == Some(generation)
        {
            // One answer is a few sentences; the cap keeps a runaway stream out of the file.
            if last.text.len() + text.len() < MATE_TEXT_MAX_BYTES {
                last.text.push('\n');
                last.text.push_str(&text);
                if let Some(mood) = mood {
                    last.mood = Some(mood.to_owned());
                }
            }
        } else {
            let model = self
                .model
                .filter(|(owner, _)| *owner == generation)
                .and_then(|(_, model)| model);
            self.push(Role::Mate, text, Some(generation));
            if let Some(last) = self.messages.last_mut()
                && last.generation == Some(generation)
            {
                last.mood = mood.map(str::to_owned);
                last.model = model.map(str::to_owned);
            }
        }
        true
    }

    fn publish(&self) -> Result<(), String> {
        if let Some(channel) = &self.subscriber {
            let _ = channel.send(self.messages.clone());
        }
        let result = (|| {
            let path = self.path.as_ref().ok_or(())?;
            std::fs::create_dir_all(path.parent().ok_or(())?).map_err(|_| ())?;
            let raw = serde_json::to_vec(&self.messages).map_err(|_| ())?;
            let temporary = path.with_extension("json.tmp");
            std::fs::write(&temporary, raw).map_err(|_| ())?;
            std::fs::rename(temporary, path).map_err(|_| ())
        })();
        result.map_err(|_| "could not save chat".to_owned())
    }
}

fn valid_text(role: Role, text: &str) -> bool {
    !text.trim().is_empty()
        && match role {
            Role::Person => text.chars().count() <= 1000,
            Role::Mate => text.len() <= MATE_TEXT_MAX_BYTES,
        }
}

fn load_history(reader: impl Read) -> Vec<Message> {
    let mut raw = Vec::new();
    if reader
        .take((HISTORY_MAX_BYTES + 1) as u64)
        .read_to_end(&mut raw)
        .is_err()
        || raw.len() > HISTORY_MAX_BYTES
    {
        return Vec::new();
    }
    let mut messages = serde_json::from_slice::<Vec<Message>>(&raw).unwrap_or_default();
    messages.retain(|message| valid_text(message.role, &message.text));
    for message in &mut messages {
        message.mood = if message.role == Role::Mate {
            message
                .mood
                .as_deref()
                .and_then(chat_mood)
                .map(str::to_owned)
        } else {
            None
        };
        message.model = if message.role == Role::Mate {
            message
                .model
                .as_deref()
                .and_then(chat_model)
                .map(str::to_owned)
        } else {
            None
        };
    }
    let excess = messages.len().saturating_sub(LIMIT);
    messages.drain(..excess);
    messages
}

/// Bind a queued chat question before the voice loop emits any of its feedback.
pub(crate) fn start_request(app: &AppHandle, epoch: u64, generation: u64) -> bool {
    app.state::<Chat>()
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .start_request(epoch, generation)
}

pub fn init(app: &AppHandle) {
    let chat = app.state::<Chat>();
    let mut history = chat.0.lock().unwrap_or_else(PoisonError::into_inner);
    history.path = app
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("chat.json"));
    history.messages = history
        .path
        .as_ref()
        .and_then(|path| std::fs::File::open(path).ok())
        .map(load_history)
        .unwrap_or_default();
}

pub fn record(app: &AppHandle, message: &AnswerMsg, locale: &str) {
    let chat = app.state::<Chat>();
    let Ok(mut history) = chat.0.lock() else {
        return;
    };
    if let Some(listening) = history.talk_state(message) {
        let _ = app.emit_to("chat", TALK_EVENT, listening);
    }
    if history.answer(message, locale) && history.publish().is_err() {
        eprintln!("chat: could not save history");
    }
}

/// A spoken question (voice.rs), recorded like a typed one; its text is never logged.
pub(crate) fn spoken(app: &AppHandle, generation: u64, text: &str) {
    let chat = app.state::<Chat>();
    let mut history = chat.0.lock().unwrap_or_else(PoisonError::into_inner);
    if history.spoken(generation, text) && history.publish().is_err() {
        eprintln!("chat: could not save history");
    }
}

pub(crate) fn open(app: &AppHandle) -> tauri::Result<()> {
    crate::presence::wake(app, false);
    let window = match app.get_webview_window("chat") {
        Some(window) => window,
        None => WebviewWindowBuilder::new(app, "chat", WebviewUrl::App("chat.html".into()))
            .title(crate::locale_text(
                &app.state::<crate::answer_stream::AnswerStream>().locale(),
                "chat.title",
            ))
            .inner_size(640.0, 720.0)
            .min_inner_size(360.0, 400.0)
            .center()
            .resizable(true)
            .decorations(true)
            .focused(false)
            .visible(false)
            .additional_browser_args(crate::overlay::BROWSER_ARGS)
            .build()?,
    };
    #[cfg(windows)]
    mm_win32::overlay::show(window.hwnd()?);
    #[cfg(not(windows))]
    window.show()?;
    Ok(())
}

#[tauri::command]
pub async fn open_chat(app: AppHandle) -> Result<(), String> {
    open(&app).map_err(|_| "could not open chat".to_owned())?;
    // The person asked for the chat to type in it: bring it to the front (an explicit open).
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window("chat")
        && let Ok(hwnd) = window.hwnd()
    {
        mm_win32::focus::activate_on_request(hwnd);
    }
    Ok(())
}

/// Snapshot and subscription share the same lock, including during concurrent answers.
#[tauri::command]
pub fn chat_subscribe(
    app: AppHandle,
    channel: Channel<Vec<Message>>,
    chat: tauri::State<'_, Chat>,
) -> String {
    let mut history = chat.0.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = channel.send(history.messages.clone());
    history.subscriber = Some(channel);
    // A chat opened during a recording shows Stop at once.
    let _ = app.emit_to("chat", TALK_EVENT, history.listening.is_some());
    app.state::<crate::answer_stream::AnswerStream>().locale()
}

/// The chat's Talk/Stop button: starts or stops a recording exactly like a click on Mate.
#[tauri::command]
pub fn chat_talk(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    if crate::voice::chat_talk(&app) {
        return Ok(());
    }
    #[cfg(not(windows))]
    let _ = app;
    Err("voice input is not running".to_owned())
}

fn question(text: &str) -> Result<String, String> {
    if text.chars().count() > 1000 || text.trim().is_empty() {
        return Err("invalid chat text".into());
    }
    Ok(text.trim().to_owned())
}

#[tauri::command]
pub async fn chat_send(app: AppHandle, text: String) -> Result<(), String> {
    let text = question(&text)?;
    let epoch = {
        let chat = app.state::<Chat>();
        let mut history = chat.0.lock().unwrap_or_else(PoisonError::into_inner);
        history.push(Role::Person, text.clone(), None);
        // Keep the session usable if the disk is unavailable.
        let _ = history.publish();
        history.epoch
    };
    #[cfg(windows)]
    if !crate::voice::ask_chat(&app, text, epoch) {
        let chat = app.state::<Chat>();
        let mut history = chat.0.lock().unwrap_or_else(PoisonError::into_inner);
        if history.epoch != epoch {
            return Ok(());
        }
        let locale = app.state::<crate::answer_stream::AnswerStream>().locale();
        let key = if crate::signin::missing(&app) {
            crate::signin::refusal_line(&app)
        } else {
            "robot.network_error"
        };
        history.push(Role::Mate, crate::locale_text(&locale, key), None);
        let _ = history.publish();
    }
    Ok(())
}

#[tauri::command]
pub fn chat_clear(chat: tauri::State<'_, Chat>) -> Result<(), String> {
    let mut history = chat.0.lock().unwrap_or_else(PoisonError::into_inner);
    // Late sentences from the answer being cleared must not recreate its bubbles.
    history.clear();
    history.publish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moods_stream_with_speech_and_legacy_history_loads() {
        let mut history = History::default();
        let say = |mood: &str| AnswerMsg::Item {
            generation: 1,
            item: Item::Say(mm_core::matescript::Say {
                id: 1,
                text: "Hello.".into(),
                bound: vec![Bound::Mood(mood.into())],
            }),
        };
        assert!(history.answer(&say("thinking"), "en"));
        assert_eq!(history.messages[0].mood, None);
        assert!(history.answer(&say("sad"), "en"));
        assert_eq!(history.messages[0].mood.as_deref(), Some("sad"));
        assert!(history.answer(&say("calm"), "en"));
        assert_eq!(history.messages[0].mood.as_deref(), Some("calm"));
        history.answer(&AnswerMsg::Cancel { generation: 1 }, "en");
        assert!(!history.answer(&say("happy"), "en"));
        assert_eq!(history.messages[0].mood.as_deref(), Some("calm"));
        let saved = serde_json::to_vec(&history.messages).unwrap();
        assert_eq!(
            load_history(saved.as_slice())[0].mood.as_deref(),
            Some("calm")
        );

        let raw = br#"[{"role":"mate","text":"old","timestamp":1},{"role":"mate","text":"new","timestamp":2,"mood":"surprised"},{"role":"person","text":"hi","timestamp":3,"mood":"sad"},{"role":"mate","text":"unknown","timestamp":4,"mood":"unknown"}]"#;
        let loaded = load_history(&raw[..]);
        assert_eq!(loaded.len(), 4);
        assert_eq!(loaded[0].mood, None);
        assert_eq!(loaded[1].mood.as_deref(), Some("surprised"));
        assert_eq!(loaded[2].mood, None);
        assert_eq!(loaded[3].mood, None);
    }

    #[test]
    fn model_chips_follow_their_answer_and_legacy_history_loads() {
        let mut history = History::default();
        let say = |generation| AnswerMsg::Item {
            generation,
            item: Item::Say(mm_core::matescript::Say {
                id: 1,
                text: "Hello.".into(),
                bound: vec![],
            }),
        };
        let served = |generation, model| AnswerMsg::Model { generation, model };
        // A lesson names its model before it speaks.
        assert!(!history.answer(&served(1, Some("muse-wonder-1.0")), "en"));
        assert!(history.answer(&say(1), "en"));
        assert_eq!(
            history.messages[0].model.as_deref(),
            Some("muse-wonder-1.0")
        );
        // An answer names it after its first sentences; an older answer's model changes nothing.
        assert!(history.answer(&say(2), "en"));
        assert_eq!(history.messages[1].model, None);
        assert!(history.answer(&served(2, Some("muse-spark-1.3")), "en"));
        assert!(!history.answer(&served(1, Some("muse-wonder-1.0")), "en"));
        assert_eq!(
            history.messages[0].model.as_deref(),
            Some("muse-wonder-1.0")
        );
        assert_eq!(history.messages[1].model.as_deref(), Some("muse-spark-1.3"));
        history.answer(&AnswerMsg::Cancel { generation: 2 }, "en");
        assert!(!history.answer(&served(2, None), "en"));
        let saved = serde_json::to_vec(&history.messages).unwrap();
        assert_eq!(
            load_history(saved.as_slice())[1].model.as_deref(),
            Some("muse-spark-1.3")
        );

        let raw = br#"[{"role":"mate","text":"old","timestamp":1},{"role":"mate","text":"new","timestamp":2,"model":"muse-wonder-1.0"},{"role":"person","text":"hi","timestamp":3,"model":"muse-spark-1.3"},{"role":"mate","text":"raw","timestamp":4,"model":"muse-spark-preview"}]"#;
        let loaded = load_history(&raw[..]);
        assert_eq!(loaded.len(), 4);
        assert_eq!(loaded[0].model, None);
        assert_eq!(loaded[1].model.as_deref(), Some("muse-wonder-1.0"));
        assert_eq!(loaded[2].model, None);
        assert_eq!(loaded[3].model, None);
        // Without a model, a message is saved as before.
        assert!(!serde_json::to_string(&loaded[0]).unwrap().contains("model"));
    }

    #[test]
    fn clear_invalidates_queued_and_started_requests() {
        let mut history = History::default();
        let queued_epoch = history.epoch;
        history.push(Role::Person, "queued".into(), None);
        history.clear();
        assert!(!history.start_request(queued_epoch, 1));
        assert!(history.messages.is_empty());

        assert!(history.start_request(history.epoch, 2));
        history.clear();
        assert!(!history.answer(
            &AnswerMsg::Line {
                generation: 2,
                key: "robot.network_error",
            },
            "en"
        ));
        assert!(history.messages.is_empty());
        assert!(history.start_request(history.epoch, 3));
        assert!(history.answer(
            &AnswerMsg::Line {
                generation: 3,
                key: "robot.network_error",
            },
            "en"
        ));
    }

    #[test]
    fn history_read_and_message_sizes_are_bounded() {
        // An infinite reader demonstrates that the read itself stops at the cap.
        assert!(load_history(std::io::repeat(b' ')).is_empty());
        let messages = vec![
            Message {
                role: Role::Mate,
                text: "x".repeat(MATE_TEXT_MAX_BYTES + 1),
                timestamp: 0,
                mood: None,
                model: None,
                generation: None,
            },
            Message {
                role: Role::Person,
                text: "x".repeat(1001),
                timestamp: 0,
                mood: None,
                model: None,
                generation: None,
            },
            Message {
                role: Role::Mate,
                text: "kept".into(),
                timestamp: 0,
                mood: None,
                model: None,
                generation: None,
            },
        ];
        let raw = serde_json::to_vec(&messages).unwrap();
        let loaded = load_history(raw.as_slice());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].text, "kept");
        let mut history = History::default();
        history.answer(
            &AnswerMsg::Item {
                generation: 1,
                item: Item::Say(mm_core::matescript::Say {
                    id: 1,
                    text: "x".repeat(MATE_TEXT_MAX_BYTES + 1),
                    bound: vec![],
                }),
            },
            "en",
        );
        assert!(history.messages.is_empty());
    }

    #[test]
    fn questions_are_nonempty_and_limited_in_unicode_characters() {
        assert!(question(" \n ").is_err());
        assert_eq!(question(" hello\nthere ").unwrap(), "hello\nthere");
        assert!(question(&"\u{1f600}".repeat(1000)).is_ok());
        assert!(question(&"\u{1f600}".repeat(1001)).is_err());
    }

    #[test]
    fn sentences_and_refusals_stream_without_audio_or_screen_data() {
        let mut history = History::default();
        history.push(Role::Person, "hello".into(), None);
        let sentence = |id, text: &str| AnswerMsg::Item {
            generation: 3,
            item: Item::Say(mm_core::matescript::Say {
                id,
                text: text.into(),
                bound: vec![],
            }),
        };
        assert!(history.answer(&sentence(1, "First."), "en"));
        assert!(history.answer(&sentence(2, "Second."), "en"));
        assert_eq!(history.messages.len(), 2);
        assert_eq!(history.messages[1].text, "First.\nSecond.");
        assert!(!history.answer(
            &AnswerMsg::State {
                generation: 3,
                state: "idle"
            },
            "en"
        ));
        for key in [
            "robot.sign_in_first",
            "robot.network_error",
            "robot.wont_press",
        ] {
            assert!(history.answer(&AnswerMsg::Line { generation: 4, key }, "ru"));
            assert!(
                history
                    .messages
                    .last()
                    .unwrap()
                    .text
                    .contains(&crate::locale_text("ru", key))
            );
        }
        history.messages.clear();
        history.cancelled = history.latest;
        assert!(!history.answer(&sentence(3, "Late."), "en"));
        assert!(history.messages.is_empty());
        assert!(history.answer(
            &AnswerMsg::Line {
                generation: 5,
                key: "robot.network_error"
            },
            "en"
        ));
    }

    #[test]
    fn history_caps_and_loaded_generations_do_not_merge() {
        let mut history = History::default();
        for _ in 0..205 {
            history.push(Role::Person, "hello".into(), None);
        }
        assert_eq!(history.messages.len(), LIMIT);
        history.push(Role::Mate, "old".into(), Some(1));
        let raw = serde_json::to_vec(&history.messages).unwrap();
        history.messages = serde_json::from_slice(&raw).unwrap();
        let line = AnswerMsg::Line {
            generation: 1,
            key: "robot.network_error",
        };
        assert!(history.answer(&line, "en"));
        assert_ne!(history.messages.last().unwrap().text, "old");
        assert!(history.answer(&line, "en"));
        assert!(history.messages.last().unwrap().text.contains('\n'));
        history.answer(&AnswerMsg::Cancel { generation: 1 }, "en");
        assert!(!history.answer(&line, "en"));
    }

    #[test]
    fn a_spoken_question_comes_before_its_answer() {
        let mut history = History::default();
        let state = |generation, state| AnswerMsg::State { generation, state };
        // Listening: the hint, levels and partial words stay out of the history.
        history.answer(&state(4, "listening"), "en");
        assert!(!history.answer(
            &AnswerMsg::Line {
                generation: 4,
                key: "robot.click_stop",
            },
            "en"
        ));
        history.answer(&state(4, "thinking"), "en");
        assert!(history.spoken(4, "  Is this page safe?\n"));
        assert!(history.answer(
            &AnswerMsg::Item {
                generation: 4,
                item: Item::Say(mm_core::matescript::Say {
                    id: 1,
                    text: "It looks safe.".into(),
                    bound: vec![],
                }),
            },
            "en"
        ));
        let roles: Vec<_> = history.messages.iter().map(|m| m.role).collect();
        assert_eq!(roles, [Role::Person, Role::Mate]);
        assert_eq!(history.messages[0].text, "Is this page safe?");
        // Silence, a cancelled recording and a cleared chat leave nothing.
        assert!(!history.spoken(5, " \n"));
        history.answer(&AnswerMsg::Cancel { generation: 5 }, "en");
        assert!(!history.spoken(5, "late"));
        history.answer(&state(6, "listening"), "en");
        history.clear();
        assert!(!history.spoken(6, "cleared"));
        assert!(history.messages.is_empty());
        // A long recording is cut to a typed question's length, never dropped.
        assert!(history.spoken(7, &"\u{1f600}".repeat(1200)));
        let kept = &history.messages[0].text;
        assert_eq!(kept.chars().count(), 1000);
        assert!(valid_text(Role::Person, kept));
    }

    #[test]
    fn the_talk_button_follows_the_newest_recording() {
        let mut history = History::default();
        let state = |generation, state| AnswerMsg::State { generation, state };
        assert_eq!(history.talk_state(&state(3, "listening")), Some(true));
        assert_eq!(history.talk_state(&state(3, "listening")), None);
        // A lesson's older generation goes on talking while the person speaks.
        assert_eq!(history.talk_state(&state(2, "idle")), None);
        assert_eq!(
            history.talk_state(&AnswerMsg::Cancel { generation: 2 }),
            None
        );
        assert_eq!(history.talk_state(&state(3, "thinking")), Some(false));
        assert_eq!(history.talk_state(&state(3, "idle")), None);
        assert_eq!(history.talk_state(&state(4, "listening")), Some(true));
        assert_eq!(
            history.talk_state(&AnswerMsg::Cancel { generation: 4 }),
            Some(false)
        );
        let page = include_str!("../../src/chat.ts");
        assert!(page.contains(&format!("\"{TALK_EVENT}\"")), "{TALK_EVENT}");
    }

    #[test]
    fn history_roundtrip_and_clear_are_local() {
        let dir = std::env::temp_dir().join(format!("metamate-chat-test-{}", std::process::id()));
        let path = dir.join("chat.json");
        let mut history = History {
            path: Some(path.clone()),
            ..History::default()
        };
        history.push(Role::Person, "<hello>\nworld".into(), None);
        history.publish().unwrap();
        let loaded: Vec<Message> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(loaded[0].text, "<hello>\nworld");
        history.messages.clear();
        history.publish().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(dir);
    }
}
