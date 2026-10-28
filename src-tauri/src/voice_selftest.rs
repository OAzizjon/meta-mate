//! `--voice-selftest FILE.wav [--proxy URL]`: the whole voice path against the dev proxy,
//! unattended, with every latency measured. The WAV is streamed in real time as if spoken
//! into the microphone (the socket opens at key-down, D44) → Muse Voice Transcribe →
//! Muse Spark with the spoken-answer prompt → MateScript timeline → MMS-TTS sentence by
//! sentence. Prints a JSON report and writes each spoken sentence next to the input as
//! `NAME.answer-N.wav`. Needs `server/run_dev.py --tts` (the invite is read from
//! `server/.dev-invite`). Exit code 0 when an answer was spoken, 1 otherwise.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mm_core::api::request::{ModelChoice, ReasoningEffort, chat_request};
use mm_core::matescript::{Item, Parser, Settings};
use mm_core::mic::Framer;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::proxy::{AsrProgress, CallKind, Proxy};

const DEFAULT_PROXY: &str = "http://127.0.0.1:8787";
const FRAME: Duration = Duration::from_millis(80);
/// MMS-TTS voices the proxy offers; another `<lang>` is spoken with the English one.
const TTS_LANGUAGES: [&str; 2] = ["en", "ru"];

/// Runs the self-test if `--voice-selftest` is among `args`; None otherwise.
pub fn run_if_requested(args: &[String]) -> Option<i32> {
    let at = args.iter().position(|arg| arg == "--voice-selftest")?;
    let Some(wav) = args.get(at + 1) else {
        eprintln!("usage: --voice-selftest FILE.wav [--proxy URL]");
        return Some(2);
    };
    let proxy = args
        .iter()
        .position(|arg| arg == "--proxy")
        .and_then(|at| args.get(at + 1))
        .map_or(DEFAULT_PROXY, String::as_str);
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("voice self-test: no runtime: {error}");
            return Some(1);
        }
    };
    let report = runtime.block_on(selftest(Path::new(wav), proxy));
    // Every sentence spoken and written, not merely attempted.
    let spoken = report["sentences"].as_array().is_some_and(|all| {
        !all.is_empty() && all.iter().all(|sentence| sentence["wav"].is_string())
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_default()
    );
    Some(if spoken && report["error"].is_null() {
        0
    } else {
        1
    })
}

fn ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Milliseconds from `from` to `to`; null when either is unknown or `to` came first.
fn between(from: Option<Instant>, to: Option<Instant>) -> Value {
    match (from, to) {
        (Some(from), Some(to)) if to >= from => json!(ms(to - from)),
        _ => Value::Null,
    }
}

async fn selftest(path: &Path, base: &str) -> Value {
    let mut report = json!({"input": path.display().to_string(), "proxy": base});
    if let Err(error) = run(path, base, &mut report).await {
        report["error"] = json!(error);
    }
    report
}

async fn run(path: &Path, base: &str, report: &mut Value) -> Result<(), String> {
    let invite_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../server/.dev-invite");
    let invite = std::fs::read_to_string(&invite_file)
        .map_err(|error| format!("no dev invite ({error}): start server/run_dev.py --tts"))?;
    let proxy = Proxy::new(base, invite.trim()).map_err(|error| error.to_string())?;
    let bytes = std::fs::read(path).map_err(|error| format!("cannot read the WAV: {error}"))?;
    let wav = mm_core::wav::read(&bytes).map_err(|error| format!("bad WAV: {error}"))?;
    let mut framer =
        Framer::new(wav.sample_rate, wav.channels).map_err(|error| error.to_string())?;
    let mut frames = framer.push(&wav.samples);
    frames.extend(framer.finish());
    report["speech_ms"] = json!(ms(FRAME * u32::try_from(frames.len()).unwrap_or(u32::MAX)));

    // Key-down: the socket opens while the "person" speaks, one frame every 80 ms.
    let key_down = Instant::now();
    let (sender, receiver) = mpsc::channel(256);
    let microphone = tokio::spawn(async move {
        let mut tick = tokio::time::interval(FRAME);
        for frame in frames {
            tick.tick().await;
            if sender.send(frame).await.is_err() {
                break;
            }
        }
        tick.tick().await;
        Instant::now()
    });
    let mut ready = None;
    let mut first_partial = None;
    let transcript = proxy
        .transcribe(receiver, |progress| match progress {
            AsrProgress::Ready => ready = Some(Instant::now()),
            AsrProgress::Partial(_) => {
                first_partial.get_or_insert_with(Instant::now);
            }
        })
        .await
        .map_err(|error| format!("speech recognition: {error}"))?;
    let key_up = microphone.await.ok();
    let heard = Instant::now();
    report["asr"] = json!({
        "ready_after_key_down_ms": between(Some(key_down), ready),
        "first_partial_after_key_down_ms": between(Some(key_down), first_partial),
        "final_after_key_up_ms": between(key_up, Some(heard)),
        "final_after_end_stream_ms": between(transcript.ended_at, Some(heard)),
        "transcript": transcript.text,
    });
    if transcript.text.is_empty() {
        return Err("nothing was recognized".into());
    }

    // The answer, parsed into the timeline as it streams; each sentence goes to TTS at once.
    let (sentences, mut to_speak) = mpsc::unbounded_channel::<(String, String, Instant)>();
    let speaker = {
        let proxy = proxy.clone();
        let stem = path.with_extension("");
        tokio::spawn(async move {
            let mut spoken = Vec::new();
            while let Some((lang, text, parsed)) = to_speak.recv().await {
                spoken.push(
                    speak(&proxy, &stem, spoken.len() + 1, &lang, &text, parsed, heard).await,
                );
            }
            spoken
        })
    };
    let mut parser = Parser::new(Settings::default());
    let mut lang = "en".to_owned();
    let mut first_text = None;
    let mut first_sentence = None;
    let mut timeline = Vec::new();
    let mut on_items = |items: Vec<Item>, timeline: &mut Vec<Value>| {
        for item in items {
            match &item {
                Item::Lang(code) => lang = code.clone(),
                Item::Say(say) => {
                    first_sentence.get_or_insert_with(Instant::now);
                    let voice = if TTS_LANGUAGES.contains(&lang.as_str()) {
                        &lang
                    } else {
                        "en"
                    };
                    let _ = sentences.send((voice.to_owned(), say.text.clone(), Instant::now()));
                }
                _ => {}
            }
            timeline.push(serde_json::to_value(&item).unwrap_or(Value::Null));
        }
    };
    let body = chat_request(
        ModelChoice::Quick,
        ReasoningEffort::Minimal,
        None,
        &transcript.text,
    );
    let answer = proxy
        .respond(CallKind::Answer, &body, |delta| {
            first_text.get_or_insert_with(Instant::now);
            on_items(parser.push(delta), &mut timeline);
        })
        .await;
    let done = Instant::now();
    on_items(parser.finish(), &mut timeline);
    drop(on_items);
    drop(sentences); // the speaker finishes the queue, then stops
    let answer = answer.map_err(|error| format!("answer: {error}"))?;
    report["answer"] = json!({
        "model": answer.model,
        "status": answer.response.status,
        "first_text_after_heard_ms": between(Some(heard), first_text),
        "first_sentence_after_heard_ms": between(Some(heard), first_sentence),
        "done_after_heard_ms": between(Some(heard), Some(done)),
        "usage": answer.response.usage.as_ref().map(|usage| json!({
            "input_tokens": usage.input_tokens,
            "output_tokens": usage.output_tokens,
            "reasoning_tokens": usage.reasoning_tokens,
        })),
        "timeline": timeline,
    });
    let spoken = speaker.await.map_err(|error| error.to_string())?;
    let first_audio = spoken
        .iter()
        .find_map(|sentence| sentence["audio_after_heard_ms"].as_u64());
    report["key_up_to_first_audio_ms"] = match (key_up, first_audio) {
        (Some(key_up), Some(audio)) => json!(ms(heard - key_up) + audio),
        _ => Value::Null,
    };
    report["sentences"] = json!(spoken);
    Ok(())
}

/// Synthesizes one sentence and writes it next to the input; a failure is reported in place.
async fn speak(
    proxy: &Proxy,
    stem: &Path,
    number: usize,
    lang: &str,
    text: &str,
    parsed: Instant,
    heard: Instant,
) -> Value {
    let started = Instant::now();
    match proxy.speak(lang, text).await {
        Ok(speech) => {
            let file = PathBuf::from(format!("{}.answer-{number}.wav", stem.display()));
            let written = std::fs::write(&file, &speech.wav).is_ok();
            json!({
                "lang": lang,
                "text": text,
                "queued_ms": ms(started - parsed),
                "tts_ms": ms(started.elapsed()),
                "synth_ms": speech.synth_ms,
                "audio_after_heard_ms": ms(Instant::now() - heard),
                "wav": if written { json!(file.display().to_string()) } else { Value::Null },
            })
        }
        Err(error) => json!({"lang": lang, "text": text, "error": error.to_string()}),
    }
}
