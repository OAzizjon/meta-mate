//! The app's side of the MetaMate proxy (server/metamate_proxy/app.py): a streamed
//! `/v1/responses`, the `/v1/asr` WebSocket and `/v1/tts`. The invite code is the only
//! credential the app holds (CLAUDE.md "Secrets"); it never reaches a log or an error.

use std::fmt;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use mm_core::api::response::{ParsedResponse, ResponseError, StreamText};
use mm_core::api::sse::{SseEvent, SseParser};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::time::{sleep_until, timeout};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The longest silence inside a reply before it counts as stalled: the first token takes
/// up to ~10 s at the default effort (D41), a cold Modal TTS container longer.
const READ_IDLE: Duration = Duration::from_secs(40);
/// One WebSocket write: a peer that stops reading must not hold a send forever.
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
/// MMS-TTS on a cold Modal container; the caller covers the wait with a cached phrase.
const TTS_TIMEOUT: Duration = Duration::from_secs(35);
const ASR_ACK_TIMEOUT: Duration = Duration::from_secs(10);
/// After `endStream` the final transcript came in 364 ms (D44).
const ASR_FINAL_TIMEOUT: Duration = Duration::from_secs(15);
/// The proxy ends a session after 90 s; a dead connection must not hold this one longer.
const ASR_SESSION_TIMEOUT: Duration = Duration::from_secs(105);
/// A JSON answer or an error body: far below this.
const JSON_MAX_BYTES: usize = 4 << 20;
/// One spoken sentence (300 characters, about 20 s of 16 kHz PCM16): far below this.
const WAV_MAX_BYTES: usize = 8 << 20;
const ASR_END_STREAM: &str = r#"{"type":"endStream"}"#;
const SSE_EVENT_MAX_BYTES: usize = 1 << 20;

/// `X-MetaMate-Call`: the proxy's output cap and model rules for the call
/// (server/metamate_proxy/policy.py `CallKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // intent, speech and computer calls come with the teacher loop (Day 3)
pub enum CallKind {
    Intent,
    Speech,
    Answer,
    Computer,
}

impl CallKind {
    fn header(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Speech => "speech",
            Self::Answer => "answer",
            Self::Computer => "computer",
        }
    }
}

#[derive(Debug)]
pub enum ProxyError {
    /// The proxy (or Meta through it) refused: the HTTP status and the proxy's message,
    /// which the proxy has already scrubbed of its key.
    Refused {
        status: u16,
        message: String,
    },
    /// The proxy closed the speech socket with this code (4400 bad handshake, 4401 unknown
    /// invite, 4429 allowance used up).
    Closed(u16),
    Network(String),
    Timeout(&'static str),
    Protocol(&'static str),
    Answer(ResponseError),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { status, message } => write!(f, "proxy refused ({status}): {message}"),
            Self::Closed(code) => write!(f, "speech socket closed ({code})"),
            Self::Network(error) => write!(f, "network: {error}"),
            Self::Timeout(what) => write!(f, "timed out: {what}"),
            Self::Protocol(what) => write!(f, "protocol: {what}"),
            Self::Answer(error) => write!(f, "answer: {error}"),
        }
    }
}

impl std::error::Error for ProxyError {}

fn network(error: impl fmt::Display) -> ProxyError {
    ProxyError::Network(error.to_string())
}

/// A finished answer: the parsed final response and the model the proxy forwarded to
/// (`X-MetaMate-Model`: Spark when a Wonder request fell back).
#[derive(Debug)]
pub struct Answer {
    pub response: ParsedResponse,
    pub model: Option<String>,
}

/// One synthesized sentence: a mono PCM16 WAV.
pub struct Speech {
    pub wav: Vec<u8>,
    /// Synthesis time on the TTS service (`X-Synth-Ms`).
    pub synth_ms: Option<u32>,
}

impl fmt::Debug for Speech {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Speech")
            .field("wav_bytes", &self.wav.len())
            .field("synth_ms", &self.synth_ms)
            .finish()
    }
}

/// What the speech socket reports while the person speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsrProgress {
    /// The handshake was acknowledged: frames sent so far are on their way.
    Ready,
    /// A partial transcript arrived (its text stays out of this event).
    Partial,
}

pub struct Transcript {
    pub text: String,
    /// When `endStream` was sent (the key-up as the recognizer saw it), if it was.
    pub ended_at: Option<Instant>,
}

impl fmt::Debug for Transcript {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transcript")
            .field("chars", &self.text.chars().count())
            .finish()
    }
}

#[derive(Clone)]
pub struct Proxy {
    http: reqwest::Client,
    base: String,
    invite: String,
}

impl fmt::Debug for Proxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Proxy")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl Proxy {
    /// `base` is the proxy's origin (`http://127.0.0.1:8787` in development).
    pub fn new(base: &str, invite: &str) -> Result<Self, ProxyError> {
        let base = base.trim_end_matches('/');
        // Parsed, not prefix-matched: "http://127.0.0.1:x@elsewhere" is another host.
        let url =
            reqwest::Url::parse(base).map_err(|_| ProxyError::Protocol("bad proxy address"))?;
        let local = url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
        if !(url.scheme() == "https" || local)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
        {
            return Err(ProxyError::Protocol(
                "the proxy must be an https origin (http only on 127.0.0.1)",
            ));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_IDLE)
            .build()
            .map_err(network)?;
        Ok(Self {
            http,
            base: base.to_owned(),
            invite: invite.to_owned(),
        })
    }

    /// Sends one Responses API request and streams the answer: `on_delta` gets each piece
    /// of output text as it arrives. Dropping the future (barge-in) closes the stream.
    pub async fn respond(
        &self,
        kind: CallKind,
        body: &Value,
        mut on_delta: impl FnMut(&str),
    ) -> Result<Answer, ProxyError> {
        let response = self
            .http
            .post(format!("{}/v1/responses", self.base))
            .bearer_auth(&self.invite)
            .header("X-MetaMate-Call", kind.header())
            .json(body)
            .send()
            .await
            .map_err(network)?;
        let model = response
            .headers()
            .get("x-metamate-model")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let response = accepted(response).await?;
        if body["stream"] != json!(true) {
            let raw = capped_body(response, JSON_MAX_BYTES).await?;
            let value: Value = serde_json::from_slice(&raw)
                .map_err(|_| ProxyError::Protocol("the answer is not JSON"))?;
            let response =
                mm_core::api::response::parse_response_value(&value).map_err(ProxyError::Answer)?;
            return Ok(Answer { response, model });
        }
        let mut parser = SseParser::new(SSE_EVENT_MAX_BYTES);
        let mut text = StreamText::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(network)?;
            let events = parser
                .push(&chunk)
                .map_err(|_| ProxyError::Protocol("an answer event is too large"))?;
            for event in events {
                if let Some(response) = feed(&mut text, &event, &mut on_delta)? {
                    return Ok(Answer { response, model });
                }
            }
        }
        let events = parser
            .finish()
            .map_err(|_| ProxyError::Protocol("an answer event is too large"))?;
        for event in events {
            if let Some(response) = feed(&mut text, &event, &mut on_delta)? {
                return Ok(Answer { response, model });
            }
        }
        Err(ProxyError::Protocol(
            "the answer ended before its final event",
        ))
    }

    /// One sentence in Mate's voice (`lang` is `en` or `ru`).
    pub async fn speak(&self, lang: &str, text: &str) -> Result<Speech, ProxyError> {
        let response = self
            .http
            .post(format!("{}/v1/tts", self.base))
            .bearer_auth(&self.invite)
            .timeout(TTS_TIMEOUT)
            .json(&json!({"lang": lang, "text": text}))
            .send()
            .await
            .map_err(network)?;
        let response = accepted(response).await?;
        let synth_ms = response
            .headers()
            .get("x-synth-ms")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        let wav = capped_body(response, WAV_MAX_BYTES).await?;
        Ok(Speech { wav, synth_ms })
    }

    /// One push-to-talk transcription. Call it at key-down: connecting takes ~1.9 s (D44),
    /// so the 80 ms PCM16 24 kHz frames sent meanwhile wait in `frames`; closing `frames`
    /// (key-up) ends the stream, and the final transcript follows.
    pub async fn transcribe(
        &self,
        mut frames: mpsc::Receiver<Vec<u8>>,
        mut on_progress: impl FnMut(AsrProgress),
    ) -> Result<Transcript, ProxyError> {
        let url = match self.base.split_once("://") {
            Some(("https", rest)) => format!("wss://{rest}/v1/asr"),
            Some(("http", rest)) => format!("ws://{rest}/v1/asr"),
            _ => return Err(ProxyError::Protocol("bad proxy address")),
        };
        let (socket, _) = timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(url))
            .await
            .map_err(|_| ProxyError::Timeout("connecting the speech socket"))?
            .map_err(network)?;
        let (mut sink, mut stream) = socket.split();
        let handshake = json!({
            "authorization": {"accessToken": format!("Bearer {}", self.invite)},
            "audioEncoding": "PCM_24KHZ",
            "mode": "PUSH_TO_TALK",
            "partialMode": "CUMULATIVE",
            "emitAudioProgress": false,
        });
        send_within(&mut sink, Message::Text(handshake.to_string().into())).await?;
        let deadline = tokio::time::Instant::now() + ASR_ACK_TIMEOUT;
        loop {
            let message = timeout_at(deadline, stream.next(), "the speech handshake").await?;
            if let Some(event) = asr_event(message)? {
                if event["sessionId"].is_string() {
                    break;
                }
                return Err(ProxyError::Protocol(
                    "the speech handshake was not acknowledged",
                ));
            }
        }
        on_progress(AsrProgress::Ready);

        let mut ended_at = None;
        let send = async {
            while let Some(frame) = frames.recv().await {
                send_within(&mut sink, Message::Binary(frame.into())).await?;
            }
            send_within(&mut sink, Message::Text(ASR_END_STREAM.into())).await?;
            Ok::<_, ProxyError>(Instant::now())
        };
        let receive = async {
            loop {
                let Some(event) = asr_event(stream.next().await)? else {
                    continue;
                };
                if event["type"] != "transcript" {
                    continue;
                }
                if event["final"] == json!(true) {
                    let text = event["transcript"].as_str().unwrap_or_default();
                    return Ok::<_, ProxyError>(text.trim().to_owned());
                }
                on_progress(AsrProgress::Partial);
            }
        };
        tokio::pin!(send, receive);
        let mut final_deadline = None;
        let session_deadline = tokio::time::Instant::now() + ASR_SESSION_TIMEOUT;
        loop {
            tokio::select! {
                sent = &mut send, if ended_at.is_none() => {
                    ended_at = Some(sent?);
                    final_deadline = Some(tokio::time::Instant::now() + ASR_FINAL_TIMEOUT);
                }
                text = &mut receive => {
                    return Ok(Transcript { text: text?, ended_at });
                }
                () = sleep_until(final_deadline.unwrap_or_else(tokio::time::Instant::now)),
                    if final_deadline.is_some() => {
                    return Err(ProxyError::Timeout("the final transcript"));
                }
                () = sleep_until(session_deadline) => {
                    return Err(ProxyError::Timeout("the speech session"));
                }
            }
        }
    }
}

/// The response if it is a success, else its scrubbed refusal message.
async fn accepted(response: reqwest::Response) -> Result<reqwest::Response, ProxyError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body: Value = match capped_body(response, JSON_MAX_BYTES).await {
        Ok(raw) => serde_json::from_slice(&raw).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    };
    let message = body["error"]["message"]
        .as_str()
        .unwrap_or("no message")
        .chars()
        .take(300)
        .collect();
    Err(ProxyError::Refused {
        status: status.as_u16(),
        message,
    })
}

/// One WebSocket message, or a timeout when the peer stopped reading.
async fn send_within(
    sink: &mut (impl futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    message: Message,
) -> Result<(), ProxyError> {
    timeout(SEND_TIMEOUT, sink.send(message))
        .await
        .map_err(|_| ProxyError::Timeout("sending to the speech socket"))?
        .map_err(network)
}

/// A whole body, but never more than `cap` bytes: the proxy is ours, yet a broken one
/// must not exhaust memory.
async fn capped_body(response: reqwest::Response, cap: usize) -> Result<Vec<u8>, ProxyError> {
    let mut body = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(network)?;
        if body.len() + chunk.len() > cap {
            return Err(ProxyError::Protocol("a reply is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn feed(
    text: &mut StreamText,
    event: &SseEvent,
    on_delta: &mut impl FnMut(&str),
) -> Result<Option<ParsedResponse>, ProxyError> {
    let before = text.text().len();
    let done = text
        .push(event.event.as_deref(), &event.data)
        .map_err(ProxyError::Answer)?;
    if let Some(delta) = text.text().get(before..).filter(|delta| !delta.is_empty()) {
        on_delta(delta);
    }
    Ok(done)
}

async fn timeout_at<T>(
    deadline: tokio::time::Instant,
    future: impl Future<Output = T>,
    what: &'static str,
) -> Result<T, ProxyError> {
    tokio::time::timeout_at(deadline, future)
        .await
        .map_err(|_| ProxyError::Timeout(what))
}

/// A JSON event from the speech socket; None for frames that carry none (ping, binary).
fn asr_event(
    message: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
) -> Result<Option<Value>, ProxyError> {
    match message {
        None => Err(ProxyError::Closed(1006)),
        Some(Err(error)) => Err(network(error)),
        Some(Ok(Message::Close(frame))) => Err(ProxyError::Closed(
            frame.map_or(1005, |CloseFrame { code, .. }| u16::from(code)),
        )),
        Some(Ok(Message::Text(text))) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|_| ProxyError::Protocol("a speech event is not JSON")),
        Some(Ok(_)) => Ok(None),
    }
}
