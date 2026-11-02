//! The live voice loop (docs/plan.md Day 2): push-to-talk → microphone → speech
//! recognition (the socket opens at key-down, D44) → Muse Spark → MateScript timeline →
//! MMS-TTS sentence by sentence, all streamed to Mate's window (answer_stream.rs). A new
//! press cancels the answer still playing (barge-in). If the answer is slow to start, a
//! cached "let me think" phrase covers the wait.
//!
//! A question about the screen ("where is", "how do I", "do it for me": D47; "is this
//! safe"; "what is on this page": D80) first gets one redacted look at the foreground window
//! (screen.rs); the answer then points, and the WebView asks `point_at` (pointing.rs) when
//! it reaches that sentence.
//!
//! For now it runs in development builds against `server/run_dev.py --tts`, whose
//! per-run invite it reads; the installed app gets its proxy and invite with Day 6.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mm_core::api::intent::{Intent, asks_repeat, guess_intent, repeat_question};
use mm_core::api::request::{
    ModelChoice, ReasoningEffort, chat_request, describe_request, point_request, safety_request,
};
use mm_core::matescript::{Bound, Item, Mode, Parser, Point, Settings};
use mm_core::ptt::{PttEvent, PttSettings};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::task::{AbortHandle, JoinHandle};

use crate::answer_stream::{AnswerMsg, AnswerStream, model_chip};
use crate::flight::Flight;
use crate::mic::{MicError, Microphone};
use crate::pointing::{Pointing, stop_pointing};
use crate::proxy::{CallKind, Proxy, ProxyError, Transcript};
use crate::ptt::{self, PttDriver};

/// How often the bubble may show a new partial transcript.
const HEARD_EVERY: Duration = Duration::from_millis(125);
/// The bubble shows the last words heard, at most this many characters.
const HEARD_MAX_CHARS: usize = 200;

/// The end of `text` that fits the bubble, cut at a character boundary.
fn heard_tail(text: &str) -> String {
    let count = text.chars().count();
    if count <= HEARD_MAX_CHARS {
        return text.to_owned();
    }
    let tail: String = text.chars().skip(count - HEARD_MAX_CHARS + 1).collect();
    format!("…{tail}")
}

/// The development proxy (server/run_dev.py).
#[cfg(debug_assertions)]
const DEV_PROXY: &str = "http://127.0.0.1:8787";
/// Silence after key-up before the cached phrase covers the wait.
const ACK_AFTER: Duration = Duration::from_millis(700);
/// 80 ms frames buffered while the speech socket connects (~1.9 s, D44): more than the
/// 60 s a press may last.
const FRAME_QUEUE: usize = 800;
/// MMS-TTS voices; another `<lang>` is spoken with the UI locale's voice.
const VOICES: [&str; 2] = ["en", "ru"];
/// Phrases that cover the wait (locales/*.json): one of the first three in turn for a
/// chat answer, so Mate does not repeat himself; the look line for the screen.
const THINKING: [&str; 3] = ["robot.thinking", "robot.thinking_2", "robot.thinking_3"];
const LOOKING: &str = "robot.looking";
/// "Is this safe?" reasons longer (`low`): the wait gets its own line.
const CHECKING: &str = "robot.checking_safety";
/// Tells Mate's window which generation is "Is this safe?": the answer stream carries no
/// intent, and the window stamps only a safety verdict (src/safety-stamp.ts).
const SAFETY_EVENT: &str = "safety-check";
/// What Mate said last (its generation and sentences), for "say that again": the
/// person's own screen only, never logged.
static LAST_ANSWER: Mutex<(u64, String)> = Mutex::new((0, String::new()));

/// "Повтори", "slower please": the last answer again, asked of the model to say it slower
/// and simpler (a creative extra of 2026-11-02); None when there is nothing to repeat.
fn repeat_of(question: &str, locale: &str) -> Option<String> {
    if !asks_repeat(question) {
        return None;
    }
    let last = LAST_ANSWER.lock().ok()?.1.clone();
    (!last.trim().is_empty()).then(|| repeat_question(&last, locale == "ru"))
}

/// How long the beam stays after the answer played out.
const POINT_LINGER: Duration = Duration::from_secs(3);
/// The longest an answer may take to play out before its pointing is cleared anyway.
const PLAY_LIMIT: Duration = Duration::from_secs(90);

/// Cached phrase WAVs by voice and locale key: the built ones (assets/phrases, `mm
/// phrases build`) and, for a wait phrase missing there, one synthesized at startup.
pub(crate) type Phrases = Arc<Mutex<HashMap<(&'static str, String), Vec<u8>>>>;

type Lessons = Arc<Mutex<Option<Lesson>>>;

#[derive(Clone)]
struct Lesson {
    generation: u64,
    words: mpsc::UnboundedSender<crate::teacher::Words>,
    abort: AbortHandle,
    speech_epoch: Arc<AtomicU64>,
}

fn live_lesson(lessons: &Lessons) -> Option<Lesson> {
    lessons
        .lock()
        .ok()?
        .as_ref()
        .filter(|lesson| !lesson.words.is_closed())
        .cloned()
}

/// The answer owns the lesson task even though subsequent presses have new sessions.
struct LessonGuard {
    lessons: Lessons,
    lesson: Lesson,
}

impl Drop for LessonGuard {
    fn drop(&mut self) {
        self.lesson.abort.abort();
        if let Ok(mut lessons) = self.lessons.lock()
            && lessons
                .as_ref()
                .is_some_and(|lesson| lesson.generation == self.lesson.generation)
        {
            *lessons = None;
        }
    }
}

/// Keeps the push-to-talk hook alive; dropping it stops the loop's input.
pub struct VoiceLoop {
    _driver: PttDriver,
}

enum Input {
    Ptt(PttEvent, Option<Instant>),
    Text(String, Option<mm_platform::TargetWindow>, Option<u64>),
    /// "What's this?" (explain.rs): a text question about one control, told the answer's
    /// generation once that answer owns the screen.
    Explain(String, oneshot::Sender<u64>),
    Line(&'static str),
    /// The chat's Talk/Stop button: a click on Mate whose recording belongs to the chat.
    ChatTalk,
}

struct TextQuestions {
    sender: mpsc::UnboundedSender<Input>,
    target: Arc<Mutex<mm_platform::RecentTargets>>,
}

/// Queues a text question on the voice loop, with the same barge-in as a new press.
pub(crate) fn ask_text(app: &AppHandle, text: String) -> bool {
    if let Some(questions) = app.try_state::<TextQuestions>() {
        let target = remembered_target(app);
        return questions
            .sender
            .send(Input::Text(text, target, None))
            .is_ok();
    }
    false
}

pub(crate) fn ask_chat(app: &AppHandle, text: String, epoch: u64) -> bool {
    app.try_state::<TextQuestions>().is_some_and(|questions| {
        questions
            .sender
            .send(Input::Text(text, remembered_target(app), Some(epoch)))
            .is_ok()
    })
}

/// Queues Mate's "What's this?" question (explain.rs) like a typed one: the same sign-in
/// gate, guest count and barge-in, always on the chat route.
pub(crate) fn ask_explain(app: &AppHandle, text: String, started: oneshot::Sender<u64>) -> bool {
    app.try_state::<TextQuestions>()
        .is_some_and(|questions| questions.sender.send(Input::Explain(text, started)).is_ok())
}

/// Starts or stops a click-to-talk recording from the chat window (chat.rs `chat_talk`).
pub(crate) fn chat_talk(app: &AppHandle) -> bool {
    app.try_state::<TextQuestions>()
        .is_some_and(|questions| questions.sender.send(Input::ChatTalk).is_ok())
}

pub(crate) fn remembered_target(app: &AppHandle) -> Option<mm_platform::TargetWindow> {
    let questions = app.try_state::<TextQuestions>()?;
    questions
        .target
        .lock()
        .ok()?
        .newest_valid(|target| mm_win32::window::remembered_target(target, &[]).is_some())
}

/// Feedback uses the loop's generations and cancels any previous answer.
pub(crate) fn show_line(app: &AppHandle, key: &'static str) {
    if let Some(questions) = app.try_state::<TextQuestions>() {
        let _ = questions.sender.send(Input::Line(key));
    }
}

/// Starts the loop when a proxy and an invite are known (`proxy_config`); None otherwise
/// (logged).
pub fn start(app: &AppHandle) -> Option<VoiceLoop> {
    let (base, invite) = proxy_config()?;
    let proxy = Proxy::new(&base, &crate::signin::invite_for_proxy(invite))
        .map_err(|error| eprintln!("voice: {error}"))
        .ok()?;
    // A kept sign-in replaces the built-in invite before the first call (D54). Before the
    // hook: without it signing in must still work (the sign-in window needs the proxy).
    crate::auth::restore(app, &proxy);
    crate::auth::watch_rejected(app, &proxy);
    // Signed out: the guest trial (12 answers an hour) until a sign-in.
    crate::signin::start_guest(app, &proxy);
    // For the outlines SAM 3.1 draws when a point is on nothing UI Automation lists, and
    // for signing in.
    app.manage(proxy.clone());
    let (events, receiver) = mpsc::unbounded_channel();
    let questions = events.clone();
    let driver = ptt::spawn_timed(PttSettings::default(), move |event, pressed_at| {
        let _ = events.send(Input::Ptt(event, pressed_at));
    })
    .map_err(|error| eprintln!("voice: push-to-talk: {error}"))
    .ok()?;
    let silencer = driver.silencer();
    app.manage(TextQuestions {
        sender: questions,
        target: Arc::default(),
    });
    tauri::async_runtime::spawn(run(app.clone(), proxy, receiver, silencer));
    Some(VoiceLoop { _driver: driver })
}

/// `--ask "question"` (development builds): the question takes the same path as a spoken
/// one, minus the microphone, and plays in Mate's window. Prints the timings and exits 0
/// once the window reports the answer played out, 1 after a minute: spike S7 (audio in
/// WebView2 without a user gesture) and the whole answer path, with no one at the laptop.
pub fn ask(app: &AppHandle, question: String) {
    let proxy = proxy_config().and_then(|(base, invite)| {
        Proxy::new(&base, &crate::signin::invite_for_proxy(invite)).ok()
    });
    let Some(proxy) = proxy else {
        eprintln!("ask: no proxy: start server/run_dev.py --tts");
        app.exit(1);
        return;
    };
    // Signing in is required here too: `--ask` never rides on the built-in invite.
    crate::auth::restore(app, &proxy);
    if crate::signin::missing(app) {
        eprintln!("ask: sign in first");
        app.exit(1);
        return;
    }
    app.manage(proxy.clone());
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
        answer(
            app.clone(),
            proxy,
            Arc::default(),
            1,
            ack_at,
            transcript,
            None,
            Arc::default(),
            None,
            None,
            None,
        )
        .await;
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

/// The proxy and the invite answers go through: in development the local proxy and the
/// invite `server/run_dev.py` wrote; a release the proxy and demo invite it was built with
/// (`METAMATE_PROXY_URL`, `METAMATE_INVITE` at build time, D51: a capped invite the proxy
/// can revoke, never a provider key).
pub(crate) fn proxy_config() -> Option<(String, String)> {
    #[cfg(debug_assertions)]
    return dev_invite().map(|invite| (DEV_PROXY.to_owned(), invite));
    #[cfg(not(debug_assertions))]
    {
        let built = option_env!("METAMATE_PROXY_URL").zip(option_env!("METAMATE_INVITE"));
        if built.is_none() {
            eprintln!("voice: this build has no proxy (METAMATE_PROXY_URL, METAMATE_INVITE)");
        }
        built.map(|(base, invite)| (base.to_owned(), invite.trim().to_owned()))
    }
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

/// One press: the microphone while the key is held, then the answer.
struct Session {
    pressed_at: Option<Instant>,
    generation: u64,
    microphone: Option<Microphone>,
    /// Set once a frame rose above the noise floor (mm_core::vad).
    heard: Arc<AtomicBool>,
    /// Started or stopped from the chat: the answer looks at the person's last app window,
    /// never at the chat in front.
    from_chat: bool,
    transcript: Option<JoinHandle<Result<Transcript, ProxyError>>>,
    /// Kept after the handle moves into the answer task: dropping a JoinHandle detaches
    /// its task, so a barge-in aborts recognition through this.
    transcript_abort: AbortHandle,
    answer: Option<JoinHandle<()>>,
}

impl Session {
    async fn abort(mut self) {
        self.transcript_abort.abort();
        if let Some(task) = self.answer.take() {
            task.abort();
            let _ = task.await;
        }
        // Dropping the microphone without stop() ends it without its last frame; its
        // destructor joins the capture thread, so not on a runtime worker.
        if let Some(microphone) = self.microphone.take() {
            tokio::task::spawn_blocking(move || drop(microphone));
        }
    }
}

/// Refusals must stop both an ordinary answer and a detached lesson before feedback.
async fn cancel_feedback(stream: &AnswerStream, current: &mut Option<Session>, lessons: &Lessons) {
    if let Some(previous) = current.take() {
        stream.send(AnswerMsg::Cancel {
            generation: previous.generation,
        });
        previous.abort().await;
    }
    if let Some(lesson) = live_lesson(lessons) {
        stream.send(AnswerMsg::Cancel {
            generation: lesson.generation,
        });
        lesson.abort.abort();
    }
}

async fn run(
    app: AppHandle,
    proxy: Proxy,
    mut events: mpsc::UnboundedReceiver<Input>,
    silencer: ptt::Silencer,
) {
    let phrases: Phrases = Arc::default();
    let resource_dir = app
        .path()
        .resolve("phrases", tauri::path::BaseDirectory::Resource)
        .ok();
    tauri::async_runtime::spawn(cache_phrases(proxy.clone(), phrases.clone(), resource_dir));
    let stream = app.state::<AnswerStream>();
    let mut current: Option<Session> = None;
    let lessons: Lessons = Arc::default();
    // Driver generations identify recordings (including VAD); answer generations also
    // include tray questions and must never collide with a later press.
    let mut next_generation = 0u64;
    let mut recording = None;
    // The chat's Talk clicked while nothing listened: the next click start is the chat's.
    let mut chat_click = false;
    let mut sampling = tokio::time::interval(Duration::from_millis(100));
    #[cfg(debug_assertions)]
    let mut sampling_wakes = crate::perf::Wakes::new("voice sampling");
    loop {
        let event = tokio::select! {
            event = events.recv() => {
                let Some(event) = event else { break };
                event
            }
            _ = sampling.tick() => {
                #[cfg(debug_assertions)]
                sampling_wakes.wake();
                sample_target(&app);
                continue;
            }
        };
        let pressed_at = match &event {
            Input::Ptt(PttEvent::Start { .. }, pressed_at) => *pressed_at,
            _ => None,
        };
        // The driver's next event answers the chat's click, whatever it is; only a click
        // start (no key-down time) makes the recording the chat's.
        let chat_claim = matches!(event, Input::Ptt(..)) && std::mem::take(&mut chat_click);
        let from_chat = chat_claim
            && matches!(event, Input::Ptt(PttEvent::Start { .. }, _))
            && pressed_at.is_none();
        // Clear also invalidates questions still waiting in the queue. Bind accepted
        // requests under the history lock before any state, refusal or answer is sent.
        if let Input::Text(_, _, Some(epoch)) = &event
            && !crate::chat::start_request(&app, *epoch, next_generation + 1)
        {
            continue;
        }
        // Anything new ends an open "What's this?" offer (explain.rs): a later click on Mate
        // must stop this recording, not ask about the control.
        if matches!(
            event,
            Input::Text(..) | Input::Line(_) | Input::Ptt(PttEvent::Start { .. }, _)
        ) {
            crate::explain::withdraw();
        }
        // Signing in is required (signin.rs): a new question opens the sign-in window
        // instead; the key's release finds no recording and is dropped below.
        if matches!(
            event,
            Input::Text(..) | Input::Explain(..) | Input::Ptt(PttEvent::Start { .. }, _)
        ) && crate::signin::missing(&app)
        {
            next_generation += 1;
            // The driver must forget a click or toggle recording too, or the next press only stops it.
            if let Some((raw, _)) = recording.take() {
                silencer.silence(raw);
            }
            cancel_feedback(&stream, &mut current, &lessons).await;
            stop_pointing(&app);
            crate::presence::wake(&app, false);
            stream.send(AnswerMsg::Line {
                generation: next_generation,
                key: crate::signin::refusal_line(&app),
            });
            stream.send(AnswerMsg::State {
                generation: next_generation,
                state: "idle",
            });
            crate::signin::open_later(&app);
            continue;
        }
        let (event, text) = match event {
            Input::ChatTalk => {
                // Exactly a click on Mate: the driver starts or stops the recording.
                match current
                    .as_mut()
                    .filter(|session| session.microphone.is_some())
                {
                    Some(session) => session.from_chat = true,
                    None => chat_click = true,
                }
                ptt::click();
                continue;
            }
            Input::Line(key) => {
                crate::presence::wake(&app, false);
                next_generation += 1;
                // The driver must forget a click or toggle recording too, or the next press only stops it.
                if let Some((raw, _)) = recording.take() {
                    silencer.silence(raw);
                }
                // A detached lesson's queued speech and timeline must stop, not only its task.
                cancel_feedback(&stream, &mut current, &lessons).await;
                stop_pointing(&app);
                say_line(&stream, &phrases, &stream.locale(), next_generation, key);
                stream.send(AnswerMsg::State {
                    generation: next_generation,
                    state: "idle",
                });
                continue;
            }
            Input::Text(text, target, _) => {
                // Home's Try and the tray's questions: a question wakes Mate like a press.
                crate::presence::wake(&app, false);
                next_generation += 1;
                // The driver must forget a click or toggle recording too, or the next press only stops it.
                if let Some((raw, _)) = recording.take() {
                    silencer.silence(raw);
                }
                (
                    PttEvent::Start {
                        generation: next_generation,
                    },
                    Some((text, target, None)),
                )
            }
            Input::Explain(text, started) => {
                crate::presence::wake(&app, false);
                next_generation += 1;
                // The driver must forget a click or toggle recording too, or the next press only stops it.
                if let Some((raw, _)) = recording.take() {
                    silencer.silence(raw);
                }
                (
                    PttEvent::Start {
                        generation: next_generation,
                    },
                    Some((text, None, Some(started))),
                )
            }
            Input::Ptt(PttEvent::Start { generation }, _) => {
                crate::presence::wake(&app, false);
                next_generation += 1;
                recording = Some((generation, next_generation));
                (
                    PttEvent::Start {
                        generation: next_generation,
                    },
                    None,
                )
            }
            Input::Ptt(PttEvent::Stop { generation, reason }, _) => {
                let Some((_, generation)) = recording.filter(|(raw, _)| *raw == generation) else {
                    continue;
                };
                (PttEvent::Stop { generation, reason }, None)
            }
            Input::Ptt(PttEvent::Cancel { generation }, _) => {
                let Some((_, generation)) = recording.filter(|(raw, _)| *raw == generation) else {
                    continue;
                };
                (PttEvent::Cancel { generation }, None)
            }
        };
        match event {
            PttEvent::Start { generation } => {
                let lesson = live_lesson(&lessons);
                // Ordinary answers are cancelled before their ring is cleared. A lesson
                // keeps its task and generation until it has heard the new words.
                if let Some(mut previous) = current.take() {
                    if lesson
                        .as_ref()
                        .is_some_and(|lesson| lesson.generation == previous.generation)
                    {
                        // Detach the owner task: its guard keeps the lesson alive.
                        previous.answer.take();
                    } else if lesson.is_none() {
                        stream.send(AnswerMsg::Cancel {
                            generation: previous.generation,
                        });
                    }
                    previous.abort().await;
                }
                if let Some(lesson) = lesson {
                    // Playback needs a sentence-only interruption: Cancel permanently
                    // rejects this generation, including all later lesson speech.
                    lesson.speech_epoch.fetch_add(1, Ordering::AcqRel);
                    if app.emit("lesson-interrupt", lesson.generation).is_err() {
                        eprintln!("voice: lesson speech interruption: delivery failed");
                    }
                } else {
                    stop_pointing(&app);
                }
                if let Some((text, target, explained)) = text {
                    // "What's this?" asks about one control, never about the window in
                    // front: the chat route, whatever its words. Any earlier ring is gone
                    // now, so explain.rs may light the control again.
                    let route = explained.map(|started| {
                        let _ = started.send(generation);
                        Intent::Chat
                    });
                    let transcript = tokio::spawn(async move {
                        Ok(Transcript {
                            text,
                            ended_at: None,
                        })
                    });
                    let transcript_abort = transcript.abort_handle();
                    stream.send(AnswerMsg::State {
                        generation,
                        state: "thinking",
                    });
                    current = Some(Session {
                        pressed_at: None,
                        generation,
                        microphone: None,
                        heard: Arc::default(),
                        from_chat: false,
                        transcript: None,
                        transcript_abort,
                        answer: Some(tokio::spawn(answer(
                            app.clone(),
                            proxy.clone(),
                            phrases.clone(),
                            generation,
                            tokio::time::Instant::now() + ACK_AFTER,
                            transcript,
                            None,
                            Arc::clone(&lessons),
                            Some(target),
                            route,
                            None,
                        ))),
                    });
                    continue;
                }
                // The voice and Glimmer wake up while the person is still talking.
                let waker = proxy.clone();
                let glimmer = crate::settings::model(&app) == crate::settings::Model::Open;
                tauri::async_runtime::spawn(async move { waker.warm(glimmer).await });
                let (frames, received) = mpsc::channel(FRAME_QUEUE);
                // Each 80 ms frame's level moves Mate's ears and, in the hands-free mode,
                // ends the recording after the speech (ptt.rs ignores it when held).
                let heard = app.clone();
                let silencer = silencer.clone();
                let driver_generation = recording.unwrap().0;
                // A microphone that fails must also end the driver's click/toggle recording,
                // or the next click would only stop this stale one.
                let release = (silencer.clone(), driver_generation);
                // A microphone that stops delivering ends a click/toggle recording as if
                // released; a held key ends at key-up.
                let on_lost = {
                    let silencer = silencer.clone();
                    move || silencer.silence(driver_generation)
                };
                let mut detector = mm_core::vad::SilenceDetector::new();
                let mut frame_ms = 0u64;
                // An empty transcript then means "say it again", not "no microphone".
                let caught = Arc::new(AtomicBool::new(false));
                let caught_level = Arc::clone(&caught);
                let on_level = move |level: f32| {
                    heard.state::<AnswerStream>().send(AnswerMsg::Level {
                        generation,
                        level: level.clamp(0.0, 1.0),
                    });
                    frame_ms += 80;
                    if detector.input(level, frame_ms).is_some() {
                        silencer.silence(driver_generation);
                    }
                    if detector.heard() {
                        caught_level.store(true, Ordering::Release);
                    }
                };
                let opened = tokio::task::spawn_blocking(move || {
                    Microphone::start(frames, on_level, on_lost)
                })
                .await;
                let microphone = match opened {
                    Ok(Ok(microphone)) => microphone,
                    Ok(Err(error)) => {
                        eprintln!("voice: microphone: {error}");
                        release.0.silence(release.1);
                        recording = None;
                        say_line(
                            &stream,
                            &phrases,
                            &stream.locale(),
                            generation,
                            microphone_line(&error),
                        );
                        stream.send(AnswerMsg::State {
                            generation,
                            state: "idle",
                        });
                        continue;
                    }
                    Err(_) => {
                        release.0.silence(release.1);
                        recording = None;
                        say_line(
                            &stream,
                            &phrases,
                            &stream.locale(),
                            generation,
                            "robot.no_microphone",
                        );
                        stream.send(AnswerMsg::State {
                            generation,
                            state: "idle",
                        });
                        continue;
                    }
                };
                // "Listening" only once the microphone really records: words spoken before
                // that would be lost while Mate already looked ready (Meta voice audit).
                stream.send(AnswerMsg::State {
                    generation,
                    state: "listening",
                });
                // Click starts carry no key-down time; keep their stop instruction visible.
                if pressed_at.is_none() {
                    stream.send(AnswerMsg::Line {
                        generation,
                        key: "robot.click_stop",
                    });
                }
                let transcriber = proxy.clone();
                // What Mate hears appears in his bubble as the person speaks (their own
                // words on their own screen; never logged), at most ~8 times a second.
                let listener = app.clone();
                let mut shown = Instant::now()
                    .checked_sub(Duration::from_secs(1))
                    .unwrap_or_else(Instant::now);
                let on_progress = move |progress: crate::proxy::AsrProgress| {
                    let crate::proxy::AsrProgress::Partial(text) = progress else {
                        return;
                    };
                    if text.is_empty() || shown.elapsed() < HEARD_EVERY {
                        return;
                    }
                    shown = Instant::now();
                    listener.state::<AnswerStream>().send(AnswerMsg::Heard {
                        generation,
                        text: heard_tail(&text),
                    });
                };
                let transcript =
                    tokio::spawn(
                        async move { transcriber.transcribe(received, on_progress).await },
                    );
                current = Some(Session {
                    pressed_at,
                    generation,
                    microphone: Some(microphone),
                    heard: caught,
                    from_chat,
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
                    // The chat is in front: look where its typed questions look.
                    let target = session.from_chat.then(|| remembered_target(&app));
                    session.answer = Some(tokio::spawn(answer(
                        app.clone(),
                        proxy.clone(),
                        phrases.clone(),
                        generation,
                        ack_at,
                        transcript,
                        session.pressed_at,
                        Arc::clone(&lessons),
                        target,
                        None,
                        Some(Arc::clone(&session.heard)),
                    )));
                }
            }
            PttEvent::Cancel { generation } => {
                if let Some(session) = current.take_if(|s| s.generation == generation) {
                    session.abort().await;
                    stream.send(AnswerMsg::State {
                        generation,
                        state: "idle",
                    });
                }
            }
        }
    }
    if let Some(lesson) = live_lesson(&lessons) {
        lesson.abort.abort();
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the answer path's own state, handed over once"
)]
async fn answer(
    app: AppHandle,
    proxy: Proxy,
    phrases: Phrases,
    generation: u64,
    ack_at: tokio::time::Instant,
    transcript: JoinHandle<Result<Transcript, ProxyError>>,
    pressed_at: Option<Instant>,
    lessons: Lessons,
    tray_target: Option<Option<mm_platform::TargetWindow>>,
    route: Option<Intent>,
    // A recording's: whether its microphone caught sound above the noise floor.
    heard: Option<Arc<AtomicBool>>,
) {
    let stream = app.state::<AnswerStream>();
    let question = match transcript.await {
        Ok(Ok(transcript)) => transcript.text,
        Ok(Err(error)) => {
            eprintln!("voice: speech recognition: {error}");
            let generation = live_lesson(&lessons).map_or(generation, |lesson| lesson.generation);
            return failed(&stream, &phrases, generation, &error);
        }
        Err(_) => return, // cancelled
    };
    if question.trim().is_empty() {
        let generation = live_lesson(&lessons).map_or(generation, |lesson| lesson.generation);
        if !(pressed_at.is_some() && crate::ptt::is_ctrl_talk_key()) {
            let caught = heard
                .as_deref()
                .is_some_and(|heard| heard.load(Ordering::Acquire));
            say_line(
                &stream,
                &phrases,
                &stream.locale(),
                generation,
                empty_transcript_line(caught),
            );
        }
        stream.send(AnswerMsg::State {
            generation,
            state: "idle",
        });
        return;
    }
    // "Say that again" during a lesson must not end it: the step stays on screen.
    if asks_repeat(&question) && live_lesson(&lessons).is_some() {
        stream.send(AnswerMsg::State {
            generation,
            state: "idle",
        });
        return;
    }
    // The lesson decides first. Recognition belongs to this press; accepted words
    // and all resulting speech still belong to the lesson's original generation.
    if let Some(lesson) = live_lesson(&lessons) {
        let (taken, reply) = oneshot::channel();
        let sent = lesson.words.send(crate::teacher::Words {
            pressed_at,
            text: question.clone(),
            taken,
        });
        if sent.is_ok() && reply.await.unwrap_or(false) {
            stream.send(AnswerMsg::State {
                generation: lesson.generation,
                state: "idle",
            });
            return;
        }
        lesson.abort.abort();
        if let Ok(mut active) = lessons.lock()
            && active
                .as_ref()
                .is_some_and(|active| active.generation == lesson.generation)
        {
            *active = None;
        }
        stream.send(AnswerMsg::Cancel {
            generation: lesson.generation,
        });
        stop_pointing(&app);
    }
    // A spoken question joins the chat history like a typed one, before its answer.
    if heard.is_some() {
        crate::chat::spoken(&app, generation, &question);
    }
    let locale = stream.locale();
    // A repeat is Mate's own words again: they never count as the person asking to open
    // something (only the person's words may, D71/D76).
    let (question, route, repeated) = match repeat_of(&question, &locale) {
        Some(again) => (again, Some(Intent::Chat), true),
        None => (question, route, false),
    };
    let intent = route.unwrap_or_else(|| guess_intent(&question));
    if intent == Intent::Chat
        && !repeated
        && crate::opening::offer_installed(&app, &proxy, generation, &question, &locale).await
    {
        return;
    }
    if tray_target == Some(None)
        && matches!(
            intent,
            Intent::How | Intent::Do | Intent::Where | Intent::Safety | Intent::Describe
        )
    {
        say_line(
            &stream,
            &phrases,
            &locale,
            generation,
            crate::screen::LookFailure::NoWindow.line(),
        );
        stream.send(AnswerMsg::State {
            generation,
            state: "idle",
        });
        return;
    }
    let target = tray_target.flatten();
    // D47: the offline guess routes at once (a classifier call would cost ~2 s); a question
    // about the screen gets one redacted look at the foreground window first.
    // Timings against key-up for "Under the hood" (debug_view.rs).
    let key_up = ack_at.checked_sub(ACK_AFTER).unwrap_or(ack_at);
    let since_key_up = |instant: tokio::time::Instant| {
        u64::try_from(instant.saturating_duration_since(key_up).as_millis()).unwrap_or(u64::MAX)
    };
    crate::debug_view::timing(
        &app,
        generation,
        "transcript",
        since_key_up(tokio::time::Instant::now()),
    );
    if matches!(intent, Intent::How | Intent::Do) {
        // Several steps: the teacher, with the person as the hand (D48).
        let own = own_windows(&app);
        let (words, received) = mpsc::unbounded_channel();
        let speech_epoch = Arc::new(AtomicU64::new(0));
        let task = tokio::spawn(crate::teacher::teach(
            app.clone(),
            proxy.clone(),
            Arc::clone(&phrases),
            generation,
            question,
            intent == Intent::Do,
            locale,
            own,
            received,
            Arc::clone(&speech_epoch),
            target,
        ));
        let lesson = Lesson {
            generation,
            words,
            abort: task.abort_handle(),
            speech_epoch,
        };
        let _guard = LessonGuard {
            lessons: Arc::clone(&lessons),
            lesson: lesson.clone(),
        };
        if let Ok(mut active) = lessons.lock() {
            *active = Some(lesson);
        }
        let _ = task.await;
        return;
    }
    // "Is this safe?" and "what is on this page?" take the where path's look and pointing;
    // only the prompt differs.
    let screen = matches!(intent, Intent::Where | Intent::Safety | Intent::Describe);
    let (body, settings, wait_phrase) = if screen {
        let own = own_windows(&app);
        let looker = app.clone();
        let looked = tokio::task::spawn_blocking(move || {
            // Mate may still be flying home: the copy waits until he is still.
            if let Some(flight) = looker.try_state::<Flight>() {
                flight.wait_idle(Duration::from_secs(1));
            }
            let wanted = || !looker.state::<AnswerStream>().is_cancelled(generation);
            // Mate's eyes sweep while he copies the window (the robot's "looking" state).
            looker.state::<AnswerStream>().send(AnswerMsg::State {
                generation,
                state: "looking",
            });
            let confirm =
                |target| crate::screen::confirm_look(&looker, generation, target, &wanted);
            let look = crate::screen::take_look_at(&own, &wanted, target, &confirm);
            looker.state::<AnswerStream>().send(AnswerMsg::State {
                generation,
                state: "thinking",
            });
            look
        });
        match looked.await {
            Ok(Ok(look)) => {
                crate::debug_view::look(
                    &app,
                    generation,
                    look.snapshot.elements.len(),
                    look.list.ids.len(),
                    look.redacted.painted,
                    since_key_up(tokio::time::Instant::now()),
                );
                // The X-ray look fills the wait for the model: rectangles only.
                let _ = crate::highlight::scan(&app, &look);
                let model = ModelChoice::from(crate::settings::model(&app));
                let (list, image) = (&look.list, &look.redacted.image);
                let (body, wait_phrase) = if intent == Intent::Safety {
                    let effort = ReasoningEffort::SAFETY_CHECK;
                    if app.emit_to("mate", SAFETY_EVENT, generation).is_err() {
                        eprintln!("voice: safety stamp: delivery failed");
                    }
                    (
                        safety_request(model, effort, &question, list, image),
                        CHECKING,
                    )
                } else if intent == Intent::Describe {
                    let effort = ReasoningEffort::SPOKEN;
                    (
                        describe_request(model, effort, &question, list, image),
                        LOOKING,
                    )
                } else {
                    let effort = ReasoningEffort::SPOKEN;
                    (
                        point_request(model, effort, &question, list, image),
                        LOOKING,
                    )
                };
                let settings = look.parser_settings(&locale, Mode::Calm);
                if let Some(pointing) = app.try_state::<Pointing>() {
                    pointing.set(generation, look);
                }
                (body, settings, wait_phrase)
            }
            Ok(Err(failure)) => {
                eprintln!("voice: look: {failure}");
                say_line(&stream, &phrases, &locale, generation, failure.line());
                stream.send(AnswerMsg::State {
                    generation,
                    state: "idle",
                });
                return;
            }
            Err(_) => return,
        }
    } else {
        let mut body = chat_request(
            ModelChoice::from(crate::settings::model(&app)),
            ReasoningEffort::SPOKEN,
            Some(&locale),
            &question,
        );
        let missing_app = (!repeated)
            .then(|| mm_core::installed_apps::requested_app(&question))
            .flatten();
        if let Some(target) = &missing_app {
            let line = crate::locale_text(&locale, "open.not_installed").replace("{name}", target);
            if let Some(instructions) = body["instructions"].as_str() {
                body["instructions"] = serde_json::Value::String(format!(
                    "{instructions}\nRust found no matching installed app. Start your answer with this literal sentence: {line}\nDo not emit an <open> tag."
                ));
            }
        }
        let settings = Settings {
            language: locale.clone(),
            open_request: (!repeated
                && missing_app.is_none()
                && mm_core::api::intent::wants_open(&question))
            .then(|| question.clone()),
            ..Settings::default()
        };
        let turn = usize::try_from(generation).unwrap_or(0) % THINKING.len();
        (body, settings, THINKING[turn])
    };
    let body = mm_core::api::profile::with_profile(body, &crate::settings::profile(&app));
    let debug_app = &app;
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
                let wav = phrase(phrases, locale, wait_phrase);
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
                    if matches!(error, ProxyError::SessionRefused) {
                        stream.send(AnswerMsg::Line {
                            generation,
                            key: error_line(&error),
                        });
                    }
                    stream.end_sentence(generation, sentence);
                }
            }
        }
    };
    let respond = async {
        let mut parser = Parser::new(settings);
        let mut lang = locale.to_owned();
        let ended = &AtomicBool::new(false);
        // A sentence whose point the look cannot resolve is never spoken as "here": it
        // becomes "I can't see that here" before its speech is made (invariant 5). A safety
        // verdict keeps its words without the beam: its red flag is often plain text (the
        // sender), and the warning matters more than the pointing. So does a description:
        // its one point is extra, the explanation is the answer.
        let look = app
            .try_state::<Pointing>()
            .and_then(|pointing| pointing.look(generation));
        let outliner = app.clone();
        let mut emit = move |items: Vec<Item>| {
            for item in items {
                let item = match item {
                    Item::Say(mut say)
                        if look.as_ref().is_some_and(|look| !resolvable(look, &say)) =>
                    {
                        if matches!(intent, Intent::Safety | Intent::Describe) {
                            say.bound
                                .retain(|bound| !matches!(bound, Bound::Point(_) | Bound::Step(_)));
                            Item::Say(say)
                        } else {
                            Item::PointRejected
                        }
                    }
                    other => other,
                };
                // A pixel only SAM can outline: ask now, while the sentence is spoken.
                if let (Item::Say(say), Some(look)) = (&item, look.as_ref()) {
                    for (x, y) in outline_pixels(look, say) {
                        if let Some(pointing) = outliner.try_state::<Pointing>() {
                            pointing.prefetch_outline(&outliner, generation, x, y);
                        }
                    }
                }
                if !matches!(item, Item::Lang(_) | Item::BadTag)
                    && !started.swap(true, Ordering::AcqRel)
                {
                    first_item.notify_one();
                    crate::debug_view::timing(
                        debug_app,
                        generation,
                        "first_item",
                        since_key_up(tokio::time::Instant::now()),
                    );
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
                        if let Ok(mut last) = LAST_ANSWER.lock() {
                            if last.0 != generation {
                                *last = (generation, String::new());
                            } else {
                                last.1.push(' ');
                            }
                            last.1.push_str(&say.text);
                        }
                    }
                    _ => {}
                }
                stream.send(AnswerMsg::Item { generation, item });
            }
        };
        let mut result = proxy
            .respond(CallKind::Answer, &body, |delta| emit(parser.push(delta)))
            .await;
        // A proxy from before D77 refuses Glimmer's id outright, before any text: the same
        // question goes to Spark, and the chip says who answered.
        if matches!(&result, Err(ProxyError::Refused { status: 400, .. }))
            && body["model"] == serde_json::json!(ModelChoice::Open)
        {
            let mut spark = body.clone();
            spark["model"] = serde_json::json!(ModelChoice::Quick);
            result = proxy
                .respond(CallKind::Answer, &spark, |delta| emit(parser.push(delta)))
                .await;
        }
        if let Ok(answer) = &result {
            stream.send(AnswerMsg::Model {
                generation,
                model: model_chip(answer.model.as_deref()),
            });
        }
        // A failed transport may end halfway through a word or instruction.
        // Only a successful response may flush that pending speech.
        if result.is_ok() {
            emit(parser.finish());
        }
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
                key: error_line(&error),
            });
        }
    };
    tokio::join!(ack, speak, respond);
    if screen {
        // The beam stays while the answer plays and a moment after, then Mate flies home.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let stream = app.state::<AnswerStream>();
            let asked = Instant::now();
            while stream.reported("done", generation).is_none() && asked.elapsed() < PLAY_LIMIT {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            tokio::time::sleep(POINT_LINGER).await;
            let current = app.try_state::<Pointing>().and_then(|p| p.generation());
            if current == Some(generation) {
                stop_pointing(&app);
            }
        });
    }
}

/// Whether every point of `say` resolves against `look` (a sentence without one does), or
/// is a pixel SAM 3.1 may outline (point_at then waits for the outline).
fn resolvable(look: &crate::screen::Look, say: &mm_core::matescript::Say) -> bool {
    let ok = |point: &Point| {
        look.resolve(point).is_some()
            || matches!(point, Point::Pixel(x, y) if crate::sam::outline_allowed(look, *x, *y))
    };
    say.bound.iter().all(|bound| match bound {
        Bound::Point(point) => ok(point),
        Bound::Step(step) => ok(&step.target),
        _ => true,
    })
}

/// The pixels of `say` that nothing in the tree resolves but SAM may outline.
fn outline_pixels(look: &crate::screen::Look, say: &mm_core::matescript::Say) -> Vec<(u32, u32)> {
    say.bound
        .iter()
        .filter_map(|bound| match bound {
            Bound::Point(point) => Some(point),
            Bound::Step(step) => Some(&step.target),
            _ => None,
        })
        .filter_map(|point| match point {
            Point::Pixel(x, y)
                if look.resolve(point).is_none() && crate::sam::outline_allowed(look, *x, *y) =>
            {
                Some((*x, *y))
            }
            _ => None,
        })
        .collect()
}

// Incremented only when overlay HWNDs are created or destroyed. Each runtime
// thread keeps a native-handle snapshot, never held across an await (HWND is !Send).
static OVERLAY_GENERATION: AtomicU64 = AtomicU64::new(1);

pub(crate) fn invalidate_overlays() {
    OVERLAY_GENERATION.fetch_add(1, Ordering::Release);
}

pub(crate) fn overlays_changed(window: &tauri::WebviewWindow) {
    invalidate_overlays();
    window.on_window_event(|event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            invalidate_overlays();
        }
    });
}

fn sample_target(app: &AppHandle) {
    std::thread_local! {
        static OWN: std::cell::RefCell<(u64, Vec<mm_win32::HWND>)> = const {
            std::cell::RefCell::new((0, Vec::new()))
        };
    }
    OWN.with(|cache| {
        let mut cache = cache.borrow_mut();
        let generation = OVERLAY_GENERATION.load(Ordering::Acquire);
        if cache.0 != generation {
            cache.1 = own_windows(app)
                .into_iter()
                .map(|handle| mm_win32::HWND(handle as *mut std::ffi::c_void))
                .collect();
            cache.0 = generation;
        }
        if let Some(target) = mm_win32::window::foreground_target(&cache.1)
            && let Ok(mut last) = app.state::<TextQuestions>().target.lock()
        {
            last.remember(target.window);
        }
    });
}

/// Overlay handles: never a target, hidden while capturing. Settings is a normal window.
fn own_windows(app: &AppHandle) -> Vec<isize> {
    // Startup is deferred: even on several monitors, no capture may restore a
    // partly prepared overlay or accept an incomplete set of highlight handles.
    if !crate::overlay::highlights_ready() {
        return Vec::new();
    }
    // All overlays or nothing: with one handle unreadable the others would still pass the
    // count check (several monitors) and that overlay would be in the picture. An empty
    // list makes the look refuse (screen.rs).
    app.webview_windows()
        .into_values()
        .filter(|window| is_overlay(window.label()))
        .map(|window| window.hwnd().map(|hwnd| hwnd.0 as isize))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default()
}

fn is_overlay(label: &str) -> bool {
    matches!(label, "mate" | "debug") || label.starts_with("highlight-")
}

/// The cached phrase `key` in the UI locale's voice.
pub(crate) fn phrase(phrases: &Phrases, locale: &str, key: &str) -> Option<Vec<u8>> {
    let cached = phrases.lock().ok()?;
    cached.get(&(voice_of(locale), key.to_owned())).cloned()
}

/// A line instead of an answer: shown in the bubble and, when it is cached, spoken.
pub(crate) fn say_line(
    stream: &AnswerStream,
    phrases: &Phrases,
    _locale: &str,
    generation: u64,
    key: &'static str,
) {
    stream.send(AnswerMsg::Line { generation, key });
    if let Some(wav) = phrase(phrases, &stream.locale(), key) {
        stream.send_wav(generation, 0, &wav);
    }
}

fn failed(stream: &AnswerStream, phrases: &Phrases, generation: u64, error: &ProxyError) {
    say_line(
        stream,
        phrases,
        &stream.locale(),
        generation,
        error_line(error),
    );
    stream.send(AnswerMsg::State {
        generation,
        state: "idle",
    });
}

/// What Mate says to an empty transcript: a microphone that caught sound worked, the words
/// were lost; one that delivered no frames or only silence is the problem.
fn empty_transcript_line(caught: bool) -> &'static str {
    if caught {
        "robot.didnt_catch"
    } else {
        "robot.no_microphone"
    }
}

/// What Mate says when the microphone does not open: what to do, where Windows says why.
fn microphone_line(error: &MicError) -> &'static str {
    match error {
        MicError::Denied(_) => "robot.mic_denied",
        MicError::Busy(_) => "robot.mic_busy",
        MicError::NoDevice | MicError::UnsupportedFormat | MicError::Device(_) => {
            "robot.no_microphone"
        }
    }
}

/// What Mate says when a call fails: a refused sign-in is not a lost connection.
pub(crate) fn error_line(error: &ProxyError) -> &'static str {
    if matches!(error, ProxyError::SessionRefused) {
        "robot.sign_in_again"
    } else {
        "robot.network_error"
    }
}

pub(crate) fn voice_of(locale: &str) -> &'static str {
    if locale == "ru" { "ru" } else { "en" }
}

/// Synthesizes the cached phrase in both voices once, at startup.
async fn cache_phrases(proxy: Proxy, phrases: Phrases, resource_dir: Option<std::path::PathBuf>) {
    let built = phrases.clone();
    if let Err(error) = tokio::task::spawn_blocking(move || {
        let resource_dir = resource_dir.filter(|dir| dir.join("manifest.json").is_file());
        #[cfg(debug_assertions)]
        let source_dir =
            Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/phrases"));
        #[cfg(not(debug_assertions))]
        let source_dir: Option<std::path::PathBuf> = None;
        if let Some(base) = mm_core::phrases::directory(
            resource_dir.as_deref(),
            source_dir.as_deref(),
            cfg!(debug_assertions),
        ) {
            load_built_phrases(&built, base);
        }
    })
    .await
    {
        eprintln!("voice: built phrases: {error}");
    }
    // Wait phrases the build does not have yet: synthesized once through the proxy.
    for key in THINKING.into_iter().chain([
        LOOKING,
        CHECKING,
        "robot.no_microphone",
        "robot.didnt_catch",
        "robot.open_browser_first",
    ]) {
        for (voice, locale) in [
            ("en", include_str!("../../locales/en.json")),
            ("ru", include_str!("../../locales/ru.json")),
        ] {
            let cached = phrases
                .lock()
                .is_ok_and(|cached| cached.contains_key(&(voice, key.to_owned())));
            if cached {
                continue;
            }
            let text = serde_json::from_str::<serde_json::Value>(locale)
                .ok()
                .and_then(|strings| strings.get(key)?.as_str().map(str::to_owned));
            let Some(text) = text else { continue };
            match proxy.speak(voice, &text).await {
                Ok(speech) => {
                    if let Ok(mut cached) = phrases.lock() {
                        cached.insert((voice, key.to_owned()), speech.wav);
                    }
                }
                Err(error) => eprintln!("voice: cached phrase: {error}"),
            }
        }
    }
}

/// The phrases `mm phrases build` made, relative to the caller's manifest directory.
fn load_built_phrases(phrases: &Phrases, base: &std::path::Path) {
    let Ok(raw) = std::fs::read_to_string(base.join("manifest.json")) else {
        return;
    };
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let Ok(mut cached) = phrases.lock() else {
        return;
    };
    for voice in VOICES {
        let Some(entries) = manifest.get(voice).and_then(|entries| entries.as_object()) else {
            continue;
        };
        for (key, entry) in entries {
            let file = entry.get("file").and_then(|file| file.as_str());
            // Only plain relative names inside the phrases folder.
            let Some(file) = file.filter(|f| !f.contains("..") && !f.starts_with(['/', '\\']))
            else {
                continue;
            };
            if let Ok(wav) = std::fs::read(base.join(file)) {
                cached.insert((voice, key.clone()), wav);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_overlay;

    #[test]
    fn failure_phrases_use_the_error_key_and_locale() {
        use super::*;

        let phrases: Phrases = Arc::default();
        for (voice, wav) in [("en", vec![1]), ("ru", vec![2])] {
            phrases
                .lock()
                .unwrap()
                .insert((voice, "robot.network_error".to_owned()), wav);
        }
        for error in [
            ProxyError::Network(String::new()),
            ProxyError::Refused {
                status: 429,
                message: String::new(),
            },
            ProxyError::Refused {
                status: 503,
                message: String::new(),
            },
        ] {
            let key = error_line(&error);
            assert_eq!(key, "robot.network_error");
            assert_eq!(phrase(&phrases, "en", key), Some(vec![1]));
            assert_eq!(phrase(&phrases, "ru", key), Some(vec![2]));
        }
        let key = error_line(&ProxyError::SessionRefused);
        assert_eq!(key, "robot.sign_in_again");
        assert_eq!(phrase(&phrases, "en", key), None);
        phrases
            .lock()
            .unwrap()
            .insert(("en", key.to_owned()), vec![3]);
        assert_eq!(phrase(&phrases, "en", key), Some(vec![3]));
    }

    #[test]
    fn an_empty_transcript_blames_the_microphone_only_when_it_caught_no_sound() {
        use super::empty_transcript_line;
        assert_eq!(empty_transcript_line(true), "robot.didnt_catch");
        assert_eq!(empty_transcript_line(false), "robot.no_microphone");
        for locale in ["en", "ru"] {
            let text = crate::locale_text(locale, empty_transcript_line(true));
            assert!(!text.is_empty() && text != "robot.didnt_catch", "{locale}");
        }
    }

    #[test]
    fn a_microphone_that_does_not_open_says_what_to_do() {
        use super::{MicError, microphone_line};
        let denied = microphone_line(&MicError::Denied(String::new()));
        let busy = microphone_line(&MicError::Busy(String::new()));
        assert_eq!(denied, "robot.mic_denied");
        assert_eq!(busy, "robot.mic_busy");
        for error in [
            MicError::NoDevice,
            MicError::UnsupportedFormat,
            MicError::Device(String::new()),
        ] {
            assert_eq!(microphone_line(&error), "robot.no_microphone");
        }
        for locale in ["en", "ru"] {
            for key in [denied, busy] {
                let text = crate::locale_text(locale, key);
                assert!(!text.is_empty() && text != key, "{locale} {key}");
            }
        }
    }

    #[tokio::test]
    async fn refusal_cancels_answer_and_detached_lesson() {
        use super::*;
        let stream = AnswerStream::default();
        let transcript = tokio::spawn(std::future::pending::<Result<Transcript, ProxyError>>());
        let transcript_abort = transcript.abort_handle();
        let answer = tokio::spawn(std::future::pending::<()>());
        let answer_abort = answer.abort_handle();
        let mut current = Some(Session {
            pressed_at: None,
            generation: 2,
            microphone: None,
            heard: Arc::default(),
            from_chat: false,
            transcript: None,
            transcript_abort: transcript_abort.clone(),
            answer: Some(answer),
        });
        let task = tokio::spawn(std::future::pending::<()>());
        let (words, _receiver) = mpsc::unbounded_channel();
        let lessons = Arc::new(Mutex::new(Some(Lesson {
            generation: 1,
            words,
            abort: task.abort_handle(),
            speech_epoch: Arc::default(),
        })));
        cancel_feedback(&stream, &mut current, &lessons).await;
        assert!(current.is_none());
        assert!(matches!(transcript.await, Err(error) if error.is_cancelled()));
        assert!(answer_abort.is_finished());
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(stream.is_cancelled(1));
        assert!(stream.is_cancelled(2));
        assert!(!stream.is_cancelled(3));
    }

    /// A press cancelled by a chord, or a failed microphone, leaves no session while the
    /// lesson it interrupted still talks: a line shown then must still cancel the lesson.
    #[tokio::test]
    async fn feedback_without_a_session_cancels_the_detached_lesson() {
        use super::*;
        let stream = AnswerStream::default();
        let task = tokio::spawn(std::future::pending::<()>());
        let (words, _receiver) = mpsc::unbounded_channel();
        let lessons = Arc::new(Mutex::new(Some(Lesson {
            generation: 3,
            words,
            abort: task.abort_handle(),
            speech_epoch: Arc::default(),
        })));
        let mut current: Option<Session> = None;
        cancel_feedback(&stream, &mut current, &lessons).await;
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(stream.is_cancelled(3));
        assert!(!stream.is_cancelled(4));
    }

    #[test]
    fn mate_window_listens_for_the_safety_event() {
        let listener = include_str!("../../src/safety-stamp.ts");
        let declared = format!("SAFETY_CHECK_EVENT = \"{}\";", super::SAFETY_EVENT);
        assert!(listener.contains(&declared), "{declared}");
    }

    #[test]
    fn only_overlays_are_hidden_during_a_look() {
        for label in ["mate", "debug", "highlight-0", "highlight-1"] {
            assert!(is_overlay(label), "{label}");
        }
        for label in ["settings", "other", "highlight", "mate-settings"] {
            assert!(!is_overlay(label), "{label}");
        }
    }
}
