//! The live voice loop (docs/plan.md Day 2): push-to-talk → microphone → speech
//! recognition (the socket opens at key-down, D44) → Muse Spark → MateScript timeline →
//! MMS-TTS sentence by sentence, all streamed to Mate's window (answer_stream.rs). A new
//! press cancels the answer still playing (barge-in). If the answer is slow to start, a
//! cached "let me think" phrase covers the wait.
//!
//! For now it runs in development builds against `server/run_dev.py --tts`, whose
//! per-run invite it reads; the installed app gets its proxy and invite with Day 6.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mm_core::api::request::{ModelChoice, ReasoningEffort, chat_request};
use mm_core::matescript::{Item, Parser, Settings};
use mm_core::ptt::{PttEvent, PttSettings};
use tauri::{AppHandle, Manager};
use tokio::sync::{Notify, mpsc};
use tokio::task::{AbortHandle, JoinHandle};

use crate::answer_stream::{AnswerMsg, AnswerStream};
use crate::mic::Microphone;
use crate::proxy::{CallKind, Proxy, ProxyError, Transcript};
use crate::ptt::{self, PttDriver};

/// The development proxy (server/run_dev.py).
const DEV_PROXY: &str = "http://127.0.0.1:8787";
/// Silence after key-up before the cached phrase covers the wait.
const ACK_AFTER: Duration = Duration::from_millis(700);
/// 80 ms frames buffered while the speech socket connects (~1.9 s, D44): more than the
/// 60 s a press may last.
const FRAME_QUEUE: usize = 800;
/// MMS-TTS voices; another `<lang>` is spoken with the UI locale's voice.
const VOICES: [&str; 2] = ["en", "ru"];
/// The cached phrases (locales/*.json), taken in turn so Mate does not repeat himself.
const ACK_KEYS: [&str; 3] = ["robot.thinking", "robot.thinking_2", "robot.thinking_3"];

/// Cached phrase WAVs by voice and index into `ACK_KEYS`.
type Phrases = Arc<Mutex<HashMap<(&'static str, usize), Vec<u8>>>>;

/// Keeps the push-to-talk hook alive; dropping it stops the loop's input.
pub struct VoiceLoop {
    _driver: PttDriver,
}

/// Starts the loop when a development proxy invite exists; None otherwise (logged).
pub fn start(app: &AppHandle) -> Option<VoiceLoop> {
    let invite = dev_invite()?;
    let proxy = Proxy::new(DEV_PROXY, &invite)
        .map_err(|error| eprintln!("voice: {error}"))
        .ok()?;
    let (events, receiver) = mpsc::unbounded_channel();
    let driver = ptt::spawn(PttSettings::default(), move |event| {
        let _ = events.send(event);
    })
    .map_err(|error| eprintln!("voice: push-to-talk: {error}"))
    .ok()?;
    tauri::async_runtime::spawn(run(app.clone(), proxy, receiver));
    Some(VoiceLoop { _driver: driver })
}

/// `--ask "question"` (development builds): the question takes the same path as a spoken
/// one, minus the microphone, and plays in Mate's window. Prints the timings and exits 0
/// once the window reports the answer played out, 1 after a minute: spike S7 (audio in
/// WebView2 without a user gesture) and the whole answer path, with no one at the laptop.
pub fn ask(app: &AppHandle, question: String) {
    let Some(proxy) = dev_invite().and_then(|invite| Proxy::new(DEV_PROXY, &invite).ok()) else {
        eprintln!("ask: no dev proxy: start server/run_dev.py --tts");
        app.exit(1);
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let stream = app.state::<AnswerStream>();
        let opened = Instant::now();
        while !stream.subscribed() && opened.elapsed() < Duration::from_secs(15) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let subscribed = opened.elapsed();
        let asked = Instant::now();
        let transcript = tokio::spawn(async move {
            Ok(Transcript {
                text: question,
                ended_at: None,
            })
        });
        let ack_at = tokio::time::Instant::now() + ACK_AFTER;
        answer(app.clone(), proxy, Arc::default(), 1, ack_at, transcript).await;
        let sent = asked.elapsed();
        let mut played = None;
        while played.is_none() && asked.elapsed() < Duration::from_secs(60) {
            played = stream.reported("done", 1);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let audio = if stream.reported("audio_running", 0).is_some() {
            "running"
        } else if stream.reported("audio_suspended", 0).is_some() {
            "suspended"
        } else {
            "unknown"
        };
        let ms = |duration: Duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        println!(
            "{}",
            serde_json::json!({
                "subscribed_after_ms": ms(subscribed),
                "audio": audio,
                "answer_sent_ms": ms(sent),
                "played_out_ms": played.map(|at| ms(at.saturating_duration_since(asked))),
                "reports": stream.reports(),
            })
        );
        app.exit(if played.is_some() && audio == "running" {
            0
        } else {
            1
        });
    });
}

#[cfg(debug_assertions)]
fn dev_invite() -> Option<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../server/.dev-invite");
    let invite = std::fs::read_to_string(path).ok()?.trim().to_owned();
    if invite.is_empty() {
        eprintln!("voice: no dev invite: start server/run_dev.py --tts");
        return None;
    }
    Some(invite)
}

#[cfg(not(debug_assertions))]
fn dev_invite() -> Option<String> {
    None
}

/// One press: the microphone while the key is held, then the answer.
struct Session {
    generation: u64,
    microphone: Option<Microphone>,
    transcript: Option<JoinHandle<Result<Transcript, ProxyError>>>,
    /// Kept after the handle moves into the answer task: dropping a JoinHandle detaches
    /// its task, so a barge-in aborts recognition through this.
    transcript_abort: AbortHandle,
    answer: Option<JoinHandle<()>>,
}

impl Session {
    fn abort(mut self) {
        self.transcript_abort.abort();
        if let Some(task) = self.answer.take() {
            task.abort();
        }
        // Dropping the microphone without stop() ends it without its last frame; its
        // destructor joins the capture thread, so not on a runtime worker.
        if let Some(microphone) = self.microphone.take() {
            tokio::task::spawn_blocking(move || drop(microphone));
        }
    }
}

async fn run(app: AppHandle, proxy: Proxy, mut events: mpsc::UnboundedReceiver<PttEvent>) {
    let phrases: Phrases = Arc::default();
    tauri::async_runtime::spawn(cache_phrases(proxy.clone(), phrases.clone()));
    let stream = app.state::<AnswerStream>();
    let mut current: Option<Session> = None;
    while let Some(event) = events.recv().await {
        match event {
            PttEvent::Start { generation } => {
                if let Some(previous) = current.take() {
                    stream.send(AnswerMsg::Cancel {
                        generation: previous.generation,
                    });
                    previous.abort();
                }
                stream.send(AnswerMsg::State {
                    generation,
                    state: "listening",
                });
                let (frames, received) = mpsc::channel(FRAME_QUEUE);
                let opened =
                    tokio::task::spawn_blocking(move || Microphone::start(frames, |_| {})).await;
                let microphone = match opened {
                    Ok(Ok(microphone)) => microphone,
                    Ok(Err(error)) => {
                        eprintln!("voice: microphone: {error}");
                        stream.send(AnswerMsg::State {
                            generation,
                            state: "idle",
                        });
                        continue;
                    }
                    Err(_) => {
                        stream.send(AnswerMsg::State {
                            generation,
                            state: "idle",
                        });
                        continue;
                    }
                };
                let transcriber = proxy.clone();
                let transcript =
                    tokio::spawn(async move { transcriber.transcribe(received, |_| {}).await });
                current = Some(Session {
                    generation,
                    microphone: Some(microphone),
                    transcript_abort: transcript.abort_handle(),
                    transcript: Some(transcript),
                    answer: None,
                });
            }
            PttEvent::Stop { generation, .. } => {
                let Some(session) = current.as_mut().filter(|s| s.generation == generation) else {
                    continue;
                };
                // The cached phrase covers the wait from key-up, not from the transcript.
                let ack_at = tokio::time::Instant::now() + ACK_AFTER;
                // stop() flushes the last frame and closes the stream: the transcript follows.
                if let Some(microphone) = session.microphone.take() {
                    tokio::task::spawn_blocking(move || microphone.stop());
                }
                stream.send(AnswerMsg::State {
                    generation,
                    state: "thinking",
                });
                if let Some(transcript) = session.transcript.take() {
                    session.answer = Some(tokio::spawn(answer(
                        app.clone(),
                        proxy.clone(),
                        phrases.clone(),
                        generation,
                        ack_at,
                        transcript,
                    )));
                }
            }
            PttEvent::Cancel { generation } => {
                if let Some(session) = current.take_if(|s| s.generation == generation) {
                    session.abort();
                    stream.send(AnswerMsg::State {
                        generation,
                        state: "idle",
                    });
                }
            }
        }
    }
}

async fn answer(
    app: AppHandle,
    proxy: Proxy,
    phrases: Phrases,
    generation: u64,
    ack_at: tokio::time::Instant,
    transcript: JoinHandle<Result<Transcript, ProxyError>>,
) {
    let stream = app.state::<AnswerStream>();
    let question = match transcript.await {
        Ok(Ok(transcript)) => transcript.text,
        Ok(Err(error)) => {
            eprintln!("voice: speech recognition: {error}");
            return failed(&stream, generation);
        }
        Err(_) => return, // cancelled
    };
    if question.is_empty() {
        stream.send(AnswerMsg::State {
            generation,
            state: "idle",
        });
        return;
    }
    let locale = stream.locale();
    // Plain references, so the three futures below can share them.
    let (stream, locale, phrases): (&AnswerStream, &str, &Phrases) = (&stream, &locale, &phrases);
    // Set by the first timeline item (or the answer's end): the cached phrase is too late then.
    let started = &AtomicBool::new(false);
    let first_item = &Notify::new();
    let (sentences, mut to_speak) = mpsc::unbounded_channel::<(String, u64, String)>();

    let ack = async {
        tokio::select! {
            biased;
            () = first_item.notified() => {}
            () = tokio::time::sleep_until(ack_at) => {
                let wav = phrases.lock().ok().and_then(|cached| {
                    let voice = voice_of(locale);
                    // This press's phrase, or any other one that is ready.
                    let turn = usize::try_from(generation).unwrap_or(0);
                    (0..ACK_KEYS.len())
                        .find_map(|offset| cached.get(&(voice, (turn + offset) % ACK_KEYS.len())))
                        .cloned()
                });
                if let Some(wav) = wav.filter(|_| !started.load(Ordering::Acquire)) {
                    stream.send_wav(generation, 0, &wav);
                }
            }
        }
    };
    let speak = async {
        while let Some((lang, sentence, text)) = to_speak.recv().await {
            match proxy.speak(&lang, &text).await {
                Ok(speech) => stream.send_wav(generation, sentence, &speech.wav),
                Err(error) => {
                    eprintln!("voice: speech synthesis: {error}");
                    stream.end_sentence(generation, sentence);
                }
            }
        }
    };
    let respond = async {
        let mut parser = Parser::new(Settings {
            language: locale.to_owned(),
            ..Settings::default()
        });
        let mut lang = locale.to_owned();
        let ended = &AtomicBool::new(false);
        let mut emit = move |items: Vec<Item>| {
            for item in items {
                if !matches!(item, Item::Lang(_) | Item::BadTag)
                    && !started.swap(true, Ordering::AcqRel)
                {
                    first_item.notify_one();
                }
                match &item {
                    Item::End => ended.store(true, Ordering::Relaxed),
                    Item::Lang(code) => lang = code.clone(),
                    Item::Say(say) => {
                        let voice = if VOICES.contains(&lang.as_str()) {
                            lang.clone()
                        } else {
                            voice_of(locale).to_owned()
                        };
                        let _ = sentences.send((voice, say.id, say.text.clone()));
                    }
                    _ => {}
                }
                stream.send(AnswerMsg::Item { generation, item });
            }
        };
        let body = chat_request(
            ModelChoice::Quick,
            ReasoningEffort::Minimal,
            None,
            &question,
        );
        let result = proxy
            .respond(CallKind::Answer, &body, |delta| emit(parser.push(delta)))
            .await;
        emit(parser.finish());
        // The parser never invents `<end>` at EOF, and the model often omits it: the
        // timeline always ends here, so the player reaches `End` and Mate goes idle.
        if !ended.load(Ordering::Relaxed) {
            emit(vec![Item::End]);
        }
        drop(emit); // closes the sentence queue: the speaker finishes and stops
        if let Err(error) = result {
            eprintln!("voice: answer: {error}");
            stream.send(AnswerMsg::Line {
                generation,
                key: "robot.network_error",
            });
        }
    };
    tokio::join!(ack, speak, respond);
}

fn failed(stream: &AnswerStream, generation: u64) {
    stream.send(AnswerMsg::Line {
        generation,
        key: "robot.network_error",
    });
    stream.send(AnswerMsg::State {
        generation,
        state: "idle",
    });
}

fn voice_of(locale: &str) -> &'static str {
    if locale == "ru" { "ru" } else { "en" }
}

/// Synthesizes the cached phrase in both voices once, at startup.
async fn cache_phrases(proxy: Proxy, phrases: Phrases) {
    for (index, key) in ACK_KEYS.into_iter().enumerate() {
        for (voice, locale) in [
            ("en", include_str!("../../locales/en.json")),
            ("ru", include_str!("../../locales/ru.json")),
        ] {
            let text = serde_json::from_str::<serde_json::Value>(locale)
                .ok()
                .and_then(|strings| strings.get(key)?.as_str().map(str::to_owned));
            let Some(text) = text else { continue };
            match proxy.speak(voice, &text).await {
                Ok(speech) => {
                    if let Ok(mut cached) = phrases.lock() {
                        cached.insert((voice, index), speech.wav);
                    }
                }
                Err(error) => eprintln!("voice: cached phrase: {error}"),
            }
        }
    }
}
