//! The live voice loop (docs/plan.md Day 2): push-to-talk → microphone → speech
//! recognition (the socket opens at key-down, D44) → Muse Spark → MateScript timeline →
//! MMS-TTS sentence by sentence, all streamed to Mate's window (answer_stream.rs). A new
//! press cancels the answer still playing (barge-in). If the answer is slow to start, a
//! cached "let me think" phrase covers the wait.
//!
//! A question about the screen ("where is", "how do I", "do it for me": D47; "is this
//! safe") first gets one redacted look at the foreground window (screen.rs); the answer
//! then points, and the WebView asks `point_at` (pointing.rs) when it reaches that sentence.
//!
//! For now it runs in development builds against `server/run_dev.py --tts`, whose
//! per-run invite it reads; the installed app gets its proxy and invite with Day 6.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mm_core::api::intent::{Intent, guess_intent};
use mm_core::api::request::{
    ModelChoice, ReasoningEffort, chat_request, point_request, safety_request,
};
use mm_core::matescript::{Bound, Item, Mode, Parser, Point, Settings};
use mm_core::ptt::{PttEvent, PttSettings};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::task::{AbortHandle, JoinHandle};

use crate::answer_stream::{AnswerMsg, AnswerStream};
use crate::flight::Flight;
use crate::mic::Microphone;
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

/// Starts the loop when a proxy and an invite are known (`proxy_config`); None otherwise
/// (logged).
pub fn start(app: &AppHandle) -> Option<VoiceLoop> {
    let (base, invite) = proxy_config()?;
    let proxy = Proxy::new(&base, &invite)
        .map_err(|error| eprintln!("voice: {error}"))
        .ok()?;
    let (events, receiver) = mpsc::unbounded_channel();
    let driver = ptt::spawn(PttSettings::default(), move |event| {
        let _ = events.send(event);
    })
    .map_err(|error| eprintln!("voice: push-to-talk: {error}"))
    .ok()?;
    let silencer = driver.silencer();
    // A kept sign-in replaces the built-in invite before the first call (D54).
    crate::auth::restore(app, &proxy);
    // For the outlines SAM 3.1 draws when a point is on nothing UI Automation lists, and
    // for signing in.
    app.manage(proxy.clone());
    tauri::async_runtime::spawn(run(app.clone(), proxy, receiver, silencer));
    Some(VoiceLoop { _driver: driver })
}

/// `--ask "question"` (development builds): the question takes the same path as a spoken
/// one, minus the microphone, and plays in Mate's window. Prints the timings and exits 0
/// once the window reports the answer played out, 1 after a minute: spike S7 (audio in
/// WebView2 without a user gesture) and the whole answer path, with no one at the laptop.
pub fn ask(app: &AppHandle, question: String) {
    let proxy = proxy_config().and_then(|(base, invite)| Proxy::new(&base, &invite).ok());
    let Some(proxy) = proxy else {
        eprintln!("ask: no proxy: start server/run_dev.py --tts");
        app.exit(1);
        return;
    };
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
            Arc::default(),
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
fn proxy_config() -> Option<(String, String)> {
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

async fn run(
    app: AppHandle,
    proxy: Proxy,
    mut events: mpsc::UnboundedReceiver<PttEvent>,
    silencer: ptt::Silencer,
) {
    let phrases: Phrases = Arc::default();
    tauri::async_runtime::spawn(cache_phrases(proxy.clone(), phrases.clone()));
    let stream = app.state::<AnswerStream>();
    let mut current: Option<Session> = None;
    let lessons: Lessons = Arc::default();
    while let Some(event) = events.recv().await {
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
                    previous.abort();
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
                stream.send(AnswerMsg::State {
                    generation,
                    state: "listening",
                });
                let (frames, received) = mpsc::channel(FRAME_QUEUE);
                // Each 80 ms frame's level moves Mate's ears and, in the hands-free mode,
                // ends the recording after the speech (ptt.rs ignores it when held).
                let heard = app.clone();
                let silencer = silencer.clone();
                let mut detector = mm_core::vad::SilenceDetector::new();
                let mut frame_ms = 0u64;
                let on_level = move |level: f32| {
                    heard.state::<AnswerStream>().send(AnswerMsg::Level {
                        generation,
                        level: level.clamp(0.0, 1.0),
                    });
                    frame_ms += 80;
                    if detector.input(level, frame_ms).is_some() {
                        silencer.silence(generation);
                    }
                };
                let opened =
                    tokio::task::spawn_blocking(move || Microphone::start(frames, on_level)).await;
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
                        Arc::clone(&lessons),
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
    if let Some(lesson) = live_lesson(&lessons) {
        lesson.abort.abort();
    }
}

async fn answer(
    app: AppHandle,
    proxy: Proxy,
    phrases: Phrases,
    generation: u64,
    ack_at: tokio::time::Instant,
    transcript: JoinHandle<Result<Transcript, ProxyError>>,
    lessons: Lessons,
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
    // The lesson decides first. Recognition belongs to this press; accepted words
    // and all resulting speech still belong to the lesson's original generation.
    if let Some(lesson) = live_lesson(&lessons) {
        let (taken, reply) = oneshot::channel();
        let sent = lesson.words.send(crate::teacher::Words {
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
    if question.is_empty() {
        stream.send(AnswerMsg::State {
            generation,
            state: "idle",
        });
        return;
    }
    let locale = stream.locale();
    // D47: the offline guess routes at once (a classifier call would cost ~2 s); a question
    // about the screen gets one redacted look at the foreground window first.
    // Development builds: timings against key-up for the debug window.
    let key_up = ack_at.checked_sub(ACK_AFTER).unwrap_or(ack_at);
    #[cfg(debug_assertions)]
    let since_key_up = |instant: tokio::time::Instant| {
        u64::try_from(instant.saturating_duration_since(key_up).as_millis()).unwrap_or(u64::MAX)
    };
    #[cfg(debug_assertions)]
    crate::debug_view::timing(
        &app,
        generation,
        "transcript",
        since_key_up(tokio::time::Instant::now()),
    );
    #[cfg(not(debug_assertions))]
    let _ = key_up;
    let intent = guess_intent(&question);
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
            locale,
            own,
            received,
            Arc::clone(&speech_epoch),
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
    // "Is this safe?" takes the where path's look and pointing; only the prompt differs.
    let screen = matches!(intent, Intent::Where | Intent::Safety);
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
            let look = crate::screen::take_look(&own, &wanted);
            looker.state::<AnswerStream>().send(AnswerMsg::State {
                generation,
                state: "thinking",
            });
            look
        });
        match looked.await {
            Ok(Ok(look)) => {
                #[cfg(debug_assertions)]
                crate::debug_view::look(
                    &app,
                    generation,
                    look.snapshot.elements.len(),
                    look.list.ids.len(),
                    look.redacted.painted,
                    since_key_up(tokio::time::Instant::now()),
                );
                let model = ModelChoice::from(crate::settings::model(&app));
                let (list, image) = (&look.list, &look.redacted.image);
                let (body, wait_phrase) = if intent == Intent::Safety {
                    let effort = ReasoningEffort::SAFETY_CHECK;
                    (
                        safety_request(model, effort, &question, list, image),
                        CHECKING,
                    )
                } else {
                    let effort = ReasoningEffort::Minimal;
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
        let body = chat_request(
            ModelChoice::from(crate::settings::model(&app)),
            ReasoningEffort::Minimal,
            None,
            &question,
        );
        let settings = Settings {
            language: locale.clone(),
            ..Settings::default()
        };
        let turn = usize::try_from(generation).unwrap_or(0) % THINKING.len();
        (body, settings, THINKING[turn])
    };
    #[cfg(debug_assertions)]
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
        // sender), and the warning matters more than the pointing.
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
                        if intent == Intent::Safety {
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
                    #[cfg(debug_assertions)]
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
                    }
                    _ => {}
                }
                stream.send(AnswerMsg::Item { generation, item });
            }
        };
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

/// Overlay handles: never a target, hidden while capturing. Settings is a normal window.
fn own_windows(app: &AppHandle) -> Vec<isize> {
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
fn phrase(phrases: &Phrases, locale: &str, key: &str) -> Option<Vec<u8>> {
    let cached = phrases.lock().ok()?;
    cached.get(&(voice_of(locale), key.to_owned())).cloned()
}

/// A line instead of an answer: shown in the bubble and, when it is cached, spoken.
pub(crate) fn say_line(
    stream: &AnswerStream,
    phrases: &Phrases,
    locale: &str,
    generation: u64,
    key: &'static str,
) {
    stream.send(AnswerMsg::Line { generation, key });
    if let Some(wav) = phrase(phrases, locale, key) {
        stream.send_wav(generation, 0, &wav);
    }
}

fn failed(stream: &AnswerStream, generation: u64) {
    // The phrases are not at hand here: the line shows in the bubble only.
    stream.send(AnswerMsg::Line {
        generation,
        key: "robot.network_error",
    });
    stream.send(AnswerMsg::State {
        generation,
        state: "idle",
    });
}

pub(crate) fn voice_of(locale: &str) -> &'static str {
    if locale == "ru" { "ru" } else { "en" }
}

/// Synthesizes the cached phrase in both voices once, at startup.
async fn cache_phrases(proxy: Proxy, phrases: Phrases) {
    load_built_phrases(&phrases);
    // Wait phrases the build does not have yet: synthesized once through the proxy.
    for key in THINKING.into_iter().chain([LOOKING, CHECKING]) {
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

/// The phrases `mm phrases build` made (assets/phrases/manifest.json). Development builds
/// read them from the source tree; the installer bundles them with Day 6.
fn load_built_phrases(phrases: &Phrases) {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/phrases");
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
    fn only_overlays_are_hidden_during_a_look() {
        for label in ["mate", "debug", "highlight-0", "highlight-1"] {
            assert!(is_overlay(label), "{label}");
        }
        for label in ["settings", "other", "highlight", "mate-settings"] {
            assert!(!is_overlay(label), "{label}");
        }
    }
}
