//! Reading a Responses API response: reasoning, the commentary line, computer calls (D41).

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct ParsedResponse {
    pub id: String,
    pub model: String,
    pub status: String,
    pub commentary: Vec<String>,
    pub computer_calls: Vec<ComputerCall>,
    pub usage: Option<Usage>,
    pub incomplete_reason: Option<String>,
    /// Number of reasoning items; reasoning content is deliberately discarded.
    pub reasoning_items: usize,
}

impl fmt::Debug for ParsedResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParsedResponse")
            .field("id", &self.id)
            .field("model", &self.model)
            .field("status", &self.status)
            .field(
                "commentary_chars",
                &self
                    .commentary
                    .iter()
                    .map(|s| s.chars().count())
                    .collect::<Vec<_>>(),
            )
            .field("computer_calls", &self.computer_calls)
            .field("usage", &self.usage)
            .field("incomplete_reason", &self.incomplete_reason)
            .field("reasoning_items", &self.reasoning_items)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputerCall {
    pub call_id: String,
    pub actions: Vec<Action>,
    pub safety_checks: Vec<SafetyCheck>,
    pub replay_receipt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SafetyCheck {
    pub id: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: Option<u64>,
}

/// All coordinates are integer pixels in the last screenshot sent to the model.
#[derive(Clone, PartialEq, Eq)]
pub enum Action {
    Click {
        x: i64,
        y: i64,
        button: String,
    },
    DoubleClick {
        x: i64,
        y: i64,
    },
    Scroll {
        x: i64,
        y: i64,
        scroll_x: i64,
        scroll_y: i64,
    },
    Type {
        text: String,
    },
    Keypress {
        keys: Vec<String>,
    },
    Drag {
        path: Vec<(i64, i64)>,
    },
    Move {
        x: i64,
        y: i64,
    },
    Wait,
    Screenshot,
    Unknown {
        kind: String,
    },
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Click { x, y, button } => f
                .debug_struct("Click")
                .field("x", x)
                .field("y", y)
                .field("button", button)
                .finish(),
            Self::DoubleClick { x, y } => f
                .debug_struct("DoubleClick")
                .field("x", x)
                .field("y", y)
                .finish(),
            Self::Scroll {
                x,
                y,
                scroll_x,
                scroll_y,
            } => f
                .debug_struct("Scroll")
                .field("x", x)
                .field("y", y)
                .field("scroll_x", scroll_x)
                .field("scroll_y", scroll_y)
                .finish(),
            Self::Type { text } => f
                .debug_struct("Type")
                .field("chars", &text.chars().count())
                .finish(),
            Self::Keypress { keys } => f.debug_struct("Keypress").field("keys", keys).finish(),
            Self::Drag { path } => f.debug_struct("Drag").field("path", path).finish(),
            Self::Move { x, y } => f.debug_struct("Move").field("x", x).field("y", y).finish(),
            Self::Wait => f.write_str("Wait"),
            Self::Screenshot => f.write_str("Screenshot"),
            Self::Unknown { kind } => f.debug_struct("Unknown").field("kind", kind).finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseError {
    InvalidJson { line: usize, column: usize },
    InvalidShape { field: &'static str },
    Api { message: String },
}

impl fmt::Display for ResponseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson { line, column } => {
                write!(f, "invalid response JSON at {line}:{column}")
            }
            Self::InvalidShape { field } => write!(f, "invalid response field: {field}"),
            Self::Api { message } => write!(f, "API error: {message}"),
        }
    }
}

impl std::error::Error for ResponseError {}

fn json(body: &str) -> Result<Value, ResponseError> {
    serde_json::from_str(body).map_err(|error| ResponseError::InvalidJson {
        line: error.line(),
        column: error.column(),
    })
}

fn decode<T: DeserializeOwned>(value: &Value, field: &'static str) -> Result<T, ResponseError> {
    serde_json::from_value(value.clone()).map_err(|_| ResponseError::InvalidShape { field })
}

fn field<T: DeserializeOwned>(value: &Value, name: &'static str) -> Result<T, ResponseError> {
    decode(&value[name], name)
}

fn api_error(value: &Value) -> Result<(), ResponseError> {
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("Unspecified API error");
        return Err(ResponseError::Api {
            message: message.trim().chars().take(200).collect(),
        });
    }
    Ok(())
}

/// Parse a response body, rather than the probe recording's outer envelope.
pub fn parse_response(body: &str) -> Result<ParsedResponse, ResponseError> {
    parse_response_value(&json(body)?)
}

/// Unknown output items and extra fields are ignored; malformed known fields are errors.
pub fn parse_response_value(value: &Value) -> Result<ParsedResponse, ResponseError> {
    api_error(value)?;
    let mut response = ParsedResponse {
        id: field(value, "id")?,
        model: field(value, "model")?,
        status: field(value, "status")?,
        commentary: Vec::new(),
        computer_calls: Vec::new(),
        usage: None,
        incomplete_reason: None,
        reasoning_items: 0,
    };
    let output: Vec<Value> = field(value, "output")?;
    for item in &output {
        match item.get("type").and_then(Value::as_str) {
            Some("reasoning") => response.reasoning_items += 1,
            Some("message") if item["phase"].is_null() || item["phase"] == "commentary" => {
                let content: Vec<Value> = field(item, "content")?;
                for part in content {
                    if matches!(part["type"].as_str(), Some("output_text" | "text")) {
                        response.commentary.push(field(&part, "text")?);
                    }
                }
            }
            Some("computer_call") => response.computer_calls.push(parse_call(item)?),
            _ => {}
        }
    }
    if let Some(usage) = value.get("usage").filter(|v| !v.is_null()) {
        response.usage = Some(Usage {
            input_tokens: field(usage, "input_tokens")?,
            output_tokens: field(usage, "output_tokens")?,
            reasoning_tokens: decode(
                &usage["output_tokens_details"]["reasoning_tokens"],
                "reasoning_tokens",
            )?,
        });
    }
    response.incomplete_reason = decode(
        &value["incomplete_details"]["reason"],
        "incomplete_details.reason",
    )?;
    Ok(response)
}

fn parse_call(value: &Value) -> Result<ComputerCall, ResponseError> {
    // Prefer the batch if both representations are supplied; never double a step.
    let actions: Vec<Value> = if !value["actions"].is_null() {
        field(value, "actions")?
    } else if value.get("action").is_some() {
        vec![value["action"].clone()]
    } else {
        return Err(ResponseError::InvalidShape { field: "actions" });
    };
    Ok(ComputerCall {
        call_id: field(value, "call_id")?,
        actions: actions.iter().map(parse_action).collect::<Result<_, _>>()?,
        safety_checks: if value["pending_safety_checks"].is_null() {
            Vec::new()
        } else {
            field(value, "pending_safety_checks")?
        },
        replay_receipt: field(value, "meta_safety_replay_receipt")?,
    })
}

fn parse_action(value: &Value) -> Result<Action, ResponseError> {
    let kind: String = field(value, "type")?;
    Ok(match kind.as_str() {
        "click" => Action::Click {
            x: field(value, "x")?,
            y: field(value, "y")?,
            button: field(value, "button")?,
        },
        "double_click" => Action::DoubleClick {
            x: field(value, "x")?,
            y: field(value, "y")?,
        },
        "scroll" => Action::Scroll {
            x: field(value, "x")?,
            y: field(value, "y")?,
            scroll_x: field(value, "scroll_x")?,
            scroll_y: field(value, "scroll_y")?,
        },
        "type" => Action::Type {
            text: field(value, "text")?,
        },
        "keypress" => Action::Keypress {
            keys: field(value, "keys")?,
        },
        "drag" => {
            let path: Vec<Value> = field(value, "path")?;
            Action::Drag {
                path: path
                    .iter()
                    .map(|p| Ok((field(p, "x")?, field(p, "y")?)))
                    .collect::<Result<_, ResponseError>>()?,
            }
        }
        "move" => Action::Move {
            x: field(value, "x")?,
            y: field(value, "y")?,
        },
        "wait" => Action::Wait,
        "screenshot" => Action::Screenshot,
        _ => Action::Unknown { kind },
    })
}

/// Consumes already-framed SSE events (event name and data JSON, not raw SSE lines).
#[derive(Default, Clone, PartialEq, Eq)]
pub struct StreamText {
    text: String,
}

impl fmt::Debug for StreamText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamText")
            .field("chars", &self.text.chars().count())
            .finish()
    }
}

impl StreamText {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Missing SSE names fall back to the JSON `type`, as in `iter_sse`.
    /// Unknown events and the transport's `[DONE]` sentinel leave the text unchanged.
    pub fn push(
        &mut self,
        event: Option<&str>,
        data: &str,
    ) -> Result<Option<ParsedResponse>, ResponseError> {
        if data.trim() == "[DONE]" {
            return Ok(None);
        }
        let value = json(data)?;
        api_error(&value)?;
        let kind = event
            .filter(|name| !name.is_empty())
            .or_else(|| value["type"].as_str());
        match kind {
            Some("response.output_text.delta") => {
                self.text.push_str(&field::<String>(&value, "delta")?);
            }
            Some("response.completed" | "response.incomplete" | "response.done") => {
                return parse_response_value(&value["response"]).map(Some);
            }
            _ => {}
        }
        Ok(None)
    }
}
