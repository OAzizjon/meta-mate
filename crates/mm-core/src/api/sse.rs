//! Incremental SSE framing for Responses API streams, without I/O.

use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

impl fmt::Debug for SseEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SseEvent")
            .field("event_bytes", &self.event.as_ref().map(String::len))
            .field("data_bytes", &self.data.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseError {
    EventTooLarge,
}

impl fmt::Display for SseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SSE event exceeds the byte limit")
    }
}

impl std::error::Error for SseError {}

/// Buffers bytes until a complete line, so chunk boundaries never affect UTF-8.
///
/// The limit counts raw bytes in nonblank lines, including their line endings,
/// comments and ignored fields. The blank dispatch line is excluded. Exceeding
/// the limit is terminal: the caller must drop the stream and this parser.
pub struct SseParser {
    max_event_bytes: usize,
    event_bytes: usize,
    line: Vec<u8>,
    event: Option<String>,
    data: Vec<u8>,
    has_data: bool,
    skip_lf: bool,
    count_lf: bool,
    failed: bool,
}

impl SseParser {
    pub fn new(max_event_bytes: usize) -> Self {
        Self {
            max_event_bytes,
            event_bytes: 0,
            line: Vec::new(),
            event: None,
            data: Vec::new(),
            has_data: false,
            skip_lf: false,
            count_lf: false,
            failed: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, SseError> {
        if self.failed {
            return Err(SseError::EventTooLarge);
        }
        let mut events = Vec::new();
        for &byte in bytes {
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    if self.count_lf {
                        self.count_byte()?;
                    }
                    continue;
                }
            }
            if matches!(byte, b'\r' | b'\n') {
                let nonblank = !self.line.is_empty();
                if nonblank {
                    self.count_byte()?;
                }
                self.end_line(&mut events);
                self.skip_lf = byte == b'\r';
                self.count_lf = nonblank;
            } else {
                self.count_byte()?;
                self.line.push(byte);
            }
        }
        Ok(events)
    }

    /// Flush a final unterminated line and dispatch if any `data` field occurred.
    /// Repeated calls return no further events. No data is dispatched after an error.
    pub fn finish(&mut self) -> Result<Vec<SseEvent>, SseError> {
        if self.failed {
            return Err(SseError::EventTooLarge);
        }
        let mut events = Vec::new();
        if !self.line.is_empty() {
            self.end_line(&mut events);
        }
        self.dispatch(&mut events);
        self.skip_lf = false;
        self.count_lf = false;
        Ok(events)
    }

    fn count_byte(&mut self) -> Result<(), SseError> {
        if self.event_bytes == self.max_event_bytes {
            self.failed = true;
            return Err(SseError::EventTooLarge);
        }
        self.event_bytes += 1;
        Ok(())
    }

    fn end_line(&mut self, events: &mut Vec<SseEvent>) {
        if self.line.is_empty() {
            self.dispatch(events);
            return;
        }
        let colon = self.line.iter().position(|&byte| byte == b':');
        let (field, value) = match colon {
            Some(index) => {
                let value = &self.line[index + 1..];
                (
                    &self.line[..index],
                    value.strip_prefix(b" ").unwrap_or(value),
                )
            }
            None => (self.line.as_slice(), &b""[..]),
        };
        match field {
            b"data" => {
                if self.has_data {
                    self.data.push(b'\n');
                }
                self.data.extend_from_slice(value);
                self.has_data = true;
            }
            b"event" => self.event = Some(String::from_utf8_lossy(value).into_owned()),
            // Includes comments (an empty field), id, retry and unknown fields.
            _ => {}
        }
        self.line.clear();
    }

    fn dispatch(&mut self, events: &mut Vec<SseEvent>) {
        if self.has_data {
            events.push(SseEvent {
                event: self.event.take(),
                data: String::from_utf8_lossy(&self.data).into_owned(),
            });
        }
        self.event = None;
        self.data.clear();
        self.has_data = false;
        self.event_bytes = 0;
    }
}
