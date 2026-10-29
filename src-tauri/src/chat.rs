//! Session chat mirrored from the answer stream; history stays in the app data folder.
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mm_core::matescript::Item;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, ipc::Channel};

use crate::answer_stream::AnswerMsg;

const LIMIT: usize = 200;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    role: Role,
    text: String,
    timestamp: u64,
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
}

#[derive(Default)]
pub struct Chat(Mutex<History>);

impl History {
    fn push(&mut self, role: Role, text: String, generation: Option<u64>) {
        self.messages.push(Message {
            role,
            text,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
            generation,
        });
        let excess = self.messages.len().saturating_sub(LIMIT);
        self.messages.drain(..excess);
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
        let (generation, text) = match message {
            AnswerMsg::Item {
                generation,
                item: Item::Say(say),
            } => (*generation, say.text.clone()),
            AnswerMsg::Item {
                generation,
                item: Item::PointRejected,
            } => (*generation, crate::locale_text(locale, "robot.cant_find")),
            AnswerMsg::Line { generation, key } => (*generation, crate::locale_text(locale, key)),
            _ => return false,
        };
        if text.trim().is_empty() || self.cancelled.is_some_and(|through| generation <= through) {
            return false;
        }
        if let Some(last) = self.messages.last_mut()
            && last.role == Role::Mate
            && last.generation == Some(generation)
        {
            last.text.push('\n');
            last.text.push_str(&text);
        } else {
            self.push(Role::Mate, text, Some(generation));
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

pub fn init(app: &AppHandle) {
    let chat = app.state::<Chat>();
    let mut history = chat.0.lock().unwrap();
    history.path = app
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("chat.json"));
    history.messages = history
        .path
        .as_ref()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|raw| serde_json::from_slice::<Vec<Message>>(&raw).ok())
        .unwrap_or_default();
    let excess = history.messages.len().saturating_sub(LIMIT);
    history.messages.drain(..excess);
}

pub fn record(app: &AppHandle, message: &AnswerMsg, locale: &str) {
    if let Ok(mut history) = app.state::<Chat>().0.lock()
        && history.answer(message, locale)
        && history.publish().is_err()
    {
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
    open(&app).map_err(|_| "could not open chat".to_owned())
}

/// Snapshot and subscription share the same lock, including during concurrent answers.
#[tauri::command]
pub fn chat_subscribe(
    app: AppHandle,
    channel: Channel<Vec<Message>>,
    chat: tauri::State<'_, Chat>,
) -> String {
    let mut history = chat.0.lock().unwrap();
    let _ = channel.send(history.messages.clone());
    history.subscriber = Some(channel);
    app.state::<crate::answer_stream::AnswerStream>().locale()
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
    {
        let chat = app.state::<Chat>();
        let mut history = chat.0.lock().unwrap();
        history.push(Role::Person, text.clone(), None);
        // Keep the session usable if the disk is unavailable.
        let _ = history.publish();
    }
    #[cfg(windows)]
    if !crate::voice::ask_text(&app, text) {
        let chat = app.state::<Chat>();
        let mut history = chat.0.lock().unwrap();
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
    let mut history = chat.0.lock().unwrap();
    history.messages.clear();
    // Late sentences from the answer being cleared must not recreate its bubbles.
    history.cancelled = history.latest;
    history.publish()
}

#[cfg(test)]
mod tests {
    use super::*;

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
