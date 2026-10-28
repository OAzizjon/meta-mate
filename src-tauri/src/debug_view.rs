//! Development debug mirror (WP20): in `cfg(debug_assertions)` builds, opens the
//! `debug` window (`debug.html`) when started with `--debug-window` and emits
//! `DebugMsg` events (`answer`, `timing`, `look`) to it whenever that window exists.
//! Release builds compile none of this module.

#![cfg(debug_assertions)]

use base64::Engine;
use mm_core::constants::APP_NAME;
use mm_core::matescript::Item;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::answer_stream::AnswerMsg;

/// The debug window's label and the event name `src/debug.ts` listens to.
const WINDOW: &str = "debug";
const EVENT: &str = "debug";
const FLAG: &str = "--debug-window";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum DebugAnswerMsg {
    State {
        generation: u64,
        state: &'static str,
    },
    Item {
        generation: u64,
        item: Item,
    },
    /// Carries sample count `samples` instead of base64 audio bytes.
    Pcm {
        generation: u64,
        sentence: u64,
        sample_rate: u32,
        eos: bool,
        samples: usize,
    },
    Cancel {
        generation: u64,
    },
    Line {
        generation: u64,
        key: &'static str,
    },
    /// Only the character count is mirrored; transcript text stays out of diagnostics.
    Heard {
        generation: u64,
        chars: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum DebugMsg {
    Answer {
        msg: DebugAnswerMsg,
    },
    Timing {
        generation: u64,
        name: String,
        ms: u64,
    },
    Look {
        generation: u64,
        elements: usize,
        listed: usize,
        painted: usize,
        ms: u64,
    },
}

/// Creates the `debug` window (900x600, normal decorations, focusable so the developer
/// can interact with it, but `.focused(false)` so it never steals focus at creation)
/// only when the app was started with `--debug-window`.
pub fn open(app: &AppHandle) {
    if !std::env::args().any(|arg| arg == FLAG) {
        return;
    }
    let result = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("debug.html".into()))
        .title(format!("{APP_NAME} Debug"))
        .inner_size(900.0, 600.0)
        .decorations(true)
        .focused(false)
        .additional_browser_args(crate::overlay::BROWSER_ARGS)
        .build();
    if let Err(error) = result {
        eprintln!("debug: window: {error}");
    }
}

/// Mirrors one `AnswerMsg` to the `debug` window if it exists.
pub fn answer(app: &AppHandle, message: &AnswerMsg) {
    // Levels come 12 times a second and say nothing about the answer.
    if matches!(message, AnswerMsg::Level { .. }) || app.get_webview_window(WINDOW).is_none() {
        return;
    }
    send(app, answer_msg(message));
}

/// Emits a `timing` event (milliseconds since key-up) to the `debug` window if it exists.
pub fn timing(app: &AppHandle, generation: u64, name: &str, ms: u64) {
    send(
        app,
        DebugMsg::Timing {
            generation,
            name: name.to_owned(),
            ms,
        },
    );
}

/// Emits a `look` summary event to the `debug` window if it exists.
pub fn look(
    app: &AppHandle,
    generation: u64,
    elements: usize,
    listed: usize,
    painted: usize,
    ms: u64,
) {
    send(
        app,
        DebugMsg::Look {
            generation,
            elements,
            listed,
            painted,
            ms,
        },
    );
}

fn send(app: &AppHandle, payload: DebugMsg) {
    if app.get_webview_window(WINDOW).is_some() {
        let _ = app.emit_to(WINDOW, EVENT, payload);
    }
}

fn answer_msg(message: &AnswerMsg) -> DebugMsg {
    let msg = match message {
        AnswerMsg::State { generation, state } => DebugAnswerMsg::State {
            generation: *generation,
            state,
        },
        AnswerMsg::Item { generation, item } => DebugAnswerMsg::Item {
            generation: *generation,
            item: item.clone(),
        },
        AnswerMsg::Pcm {
            generation,
            sentence,
            sample_rate,
            eos,
            samples_b64,
        } => DebugAnswerMsg::Pcm {
            generation: *generation,
            sentence: *sentence,
            sample_rate: *sample_rate,
            eos: *eos,
            samples: pcm_samples(samples_b64),
        },
        AnswerMsg::Cancel { generation } => DebugAnswerMsg::Cancel {
            generation: *generation,
        },
        AnswerMsg::Line { generation, key } => DebugAnswerMsg::Line {
            generation: *generation,
            key,
        },
        AnswerMsg::Heard { generation, text } => DebugAnswerMsg::Heard {
            generation: *generation,
            chars: text.chars().count(),
        },
        // Never mirrored (`answer` skips them); shown as a cancel-free no-op state.
        AnswerMsg::Level { generation, .. } => DebugAnswerMsg::State {
            generation: *generation,
            state: "listening",
        },
    };
    DebugMsg::Answer { msg }
}

/// Decoded 16-bit mono sample count of a base64 PCM payload.
fn pcm_samples(samples_b64: &str) -> usize {
    base64::engine::general_purpose::STANDARD
        .decode(samples_b64)
        .map_or(0, |bytes| bytes.len() / 2)
}

#[cfg(test)]
mod tests {
    use mm_core::matescript::{Bound, Point, Say};
    use serde_json::json;

    use super::*;

    #[test]
    fn answer_messages_serialize_to_the_debug_wire_shape() {
        let state = answer_msg(&AnswerMsg::State {
            generation: 1,
            state: "listening",
        });
        assert_eq!(
            serde_json::to_value(&state).ok(),
            Some(json!({
                "type": "answer",
                "msg": {
                    "type": "state",
                    "generation": 1,
                    "state": "listening"
                }
            }))
        );

        let item = answer_msg(&AnswerMsg::Item {
            generation: 1,
            item: Item::Say(Say {
                id: 1,
                text: "It's right here.".into(),
                bound: vec![Bound::Point(Point::Id("e12".into()))],
            }),
        });
        assert_eq!(
            serde_json::to_value(&item).ok(),
            Some(json!({
                "type": "answer",
                "msg": {
                    "type": "item",
                    "generation": 1,
                    "item": {
                        "kind": "say",
                        "value": {
                            "id": 1,
                            "text": "It's right here.",
                            "bound": [{"kind": "point", "value": {"kind": "id", "value": "e12"}}]
                        }
                    }
                }
            }))
        );

        // 3 samples = 6 bytes of PCM16LE; wire carries sample count instead of samplesB64.
        let b64 = base64::engine::general_purpose::STANDARD.encode([0u8, 0, 1, 0, 255, 127]);
        let pcm = answer_msg(&AnswerMsg::Pcm {
            generation: 2,
            sentence: 1,
            sample_rate: 16_000,
            eos: true,
            samples_b64: b64,
        });
        assert_eq!(
            serde_json::to_value(&pcm).ok(),
            Some(json!({
                "type": "answer",
                "msg": {
                    "type": "pcm",
                    "generation": 2,
                    "sentence": 1,
                    "sampleRate": 16000,
                    "eos": true,
                    "samples": 3
                }
            }))
        );

        let cancel = answer_msg(&AnswerMsg::Cancel { generation: 3 });
        assert_eq!(
            serde_json::to_value(&cancel).ok(),
            Some(json!({
                "type": "answer",
                "msg": {
                    "type": "cancel",
                    "generation": 3
                }
            }))
        );

        let line = answer_msg(&AnswerMsg::Line {
            generation: 4,
            key: "robot.cant_find",
        });
        assert_eq!(
            serde_json::to_value(&line).ok(),
            Some(json!({
                "type": "answer",
                "msg": {
                    "type": "line",
                    "generation": 4,
                    "key": "robot.cant_find"
                }
            }))
        );
    }

    #[test]
    fn heard_mirrors_only_character_count() {
        let heard = answer_msg(&AnswerMsg::Heard {
            generation: 5,
            text: "Private \u{1f4a1}".into(),
        });
        assert_eq!(
            serde_json::to_value(&heard).ok(),
            Some(json!({
                "type": "answer",
                "msg": { "type": "heard", "generation": 5, "chars": 9 }
            }))
        );
        assert!(!format!("{heard:?}").contains("Private"));
    }

    #[test]
    fn timing_and_look_serialize_to_the_debug_wire_shape() {
        let timing = DebugMsg::Timing {
            generation: 1,
            name: "asr_final".into(),
            ms: 350,
        };
        assert_eq!(
            serde_json::to_value(&timing).ok(),
            Some(json!({
                "type": "timing",
                "generation": 1,
                "name": "asr_final",
                "ms": 350
            }))
        );

        let look = DebugMsg::Look {
            generation: 1,
            elements: 142,
            listed: 35,
            painted: 2,
            ms: 180,
        };
        assert_eq!(
            serde_json::to_value(&look).ok(),
            Some(json!({
                "type": "look",
                "generation": 1,
                "elements": 142,
                "listed": 35,
                "painted": 2,
                "ms": 180
            }))
        );
    }
}
