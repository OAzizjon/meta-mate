use mm_core::api::sse::{SseError, SseEvent, SseParser};
use proptest::prelude::*;

fn event(name: Option<&str>, data: &str) -> SseEvent {
    SseEvent {
        event: name.map(str::to_owned),
        data: data.to_owned(),
    }
}

fn parse(bytes: &[u8]) -> Vec<SseEvent> {
    let mut parser = SseParser::new(4096);
    let mut events = parser.push(bytes).unwrap();
    events.extend(parser.finish().unwrap());
    events
}

#[test]
fn probe_multiline_comments_and_final_event() {
    let body = b": heartbeat\r\n\r\nevent: response.output_text.delta\r\n\
                 data: first\r\n: comment\r\ndata: second\r\n\r\n\
                 data: without event\n\nevent: final\ndata: last";
    assert_eq!(
        parse(body),
        [
            event(Some("response.output_text.delta"), "first\nsecond"),
            event(None, "without event"),
            event(Some("final"), "last"),
        ]
    );
}

#[test]
fn all_line_endings_and_every_single_split() {
    for ending in ["\n", "\r\n", "\r"] {
        let body =
            format!("event: delta{ending}data: hé🙂{ending}{ending}data: end{ending}{ending}");
        let expected = vec![event(Some("delta"), "hé🙂"), event(None, "end")];
        // Covers splits within CRLF, UTF-8 characters and the data: field name.
        for split in 0..=body.len() {
            let mut parser = SseParser::new(4096);
            let mut actual = parser.push(&body.as_bytes()[..split]).unwrap();
            actual.extend(parser.push(&[]).unwrap());
            actual.extend(parser.push(&body.as_bytes()[split..]).unwrap());
            actual.extend(parser.finish().unwrap());
            assert_eq!(actual, expected, "ending {ending:?}, split {split}");
        }
        let mut parser = SseParser::new(4096);
        let mut actual = Vec::new();
        for byte in body.as_bytes() {
            actual.extend(parser.push(&[*byte]).unwrap());
        }
        actual.extend(parser.finish().unwrap());
        assert_eq!(actual, expected);
    }
}

#[test]
fn fields_comments_spaces_and_empty_data_follow_sse_rules() {
    assert_eq!(
        parse(
            b"event: discarded\nid: 1\nretry: 10\n: comment\n\n\
                data:  two spaces\ndata:\ndata\ndata:\ttab\nunknown: ignored\n\n\
                event: first\nevent:  last \ndata: yes\n\n\
                event:\ndata:\n\n"
        ),
        [
            event(None, " two spaces\n\n\n\ttab"),
            event(Some(" last "), "yes"),
            event(Some(""), ""),
        ]
    );
}

#[test]
fn invalid_utf8_is_lossy_and_never_an_error() {
    assert_eq!(
        parse(b"event: \xff\ndata: a\xf0\x9f\ndata: \x80z\n\n"),
        [event(Some("�"), "a�\n�z")]
    );
}

#[test]
fn size_limit_is_incremental_inclusive_and_resets_between_events() {
    let mut parser = SseParser::new(8);
    assert_eq!(parser.push(b"data: x\n\n").unwrap(), [event(None, "x")]);
    assert_eq!(parser.push(b"data: y\n\n").unwrap(), [event(None, "y")]);
    assert!(parser.push(b"data: zz").unwrap().is_empty());
    assert_eq!(parser.push(b"z"), Err(SseError::EventTooLarge));
    assert_eq!(parser.push(b"\n\n"), Err(SseError::EventTooLarge));
    assert_eq!(parser.finish(), Err(SseError::EventTooLarge));

    // Also bound ignored fields and unterminated lines, not only retained data.
    for bytes in [&b":12345678"[..], &b"event: xx\ndata: x\n\n"[..]] {
        assert_eq!(SseParser::new(8).push(bytes), Err(SseError::EventTooLarge));
    }
    let mut parser = SseParser::new(8);
    assert!(parser.push(b"data: x\r").unwrap().is_empty());
    assert_eq!(parser.push(b"\n"), Err(SseError::EventTooLarge));

    let mut parser = SseParser::new(0);
    assert!(parser.push(b"\r\n\n\r").unwrap().is_empty());
    assert_eq!(parser.push(b"d"), Err(SseError::EventTooLarge));
}

#[test]
fn finish_flushes_only_events_with_data_and_is_idempotent() {
    for bytes in [
        &b"data: last"[..],
        &b"data: last\n"[..],
        &b"data: last\r"[..],
    ] {
        let mut parser = SseParser::new(4096);
        assert!(parser.push(bytes).unwrap().is_empty());
        assert_eq!(parser.finish().unwrap(), [event(None, "last")]);
        assert!(parser.finish().unwrap().is_empty());
    }
    for bytes in [&b""[..], &b"event: name"[..], &b": heartbeat\n"[..]] {
        let mut parser = SseParser::new(4096);
        assert!(parser.push(bytes).unwrap().is_empty());
        assert!(parser.finish().unwrap().is_empty());
    }
    assert_eq!(parse(b"data:"), [event(None, "")]);
}

#[test]
fn debug_shows_lengths_without_spoken_text_or_event_names() {
    let value = event(Some("private event"), "private spoken text");
    assert_eq!(
        format!("{value:?}"),
        "SseEvent { event_bytes: Some(13), data_bytes: 19 }"
    );
}

// Synthetic Responses API stream with mixed endings and an unterminated final event.
const STREAM: &str = concat!(
    ": heartbeat\r\n\r\n",
    "event: response.output_text.delta\r\n",
    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hé🙂\"}\r\n\r\n",
    "event: response.completed\r",
    "data: {\"type\":\"response.completed\",\r",
    "data: \"response\":{\"status\":\"completed\"}}\r\r",
    "data: [DONE]"
);

proptest! {
    #[test]
    fn arbitrary_chunking_matches_one_push(
        cuts in prop::collection::vec(0usize..=STREAM.len(), 0..128)
    ) {
        let expected = parse(STREAM.as_bytes());
        let mut cuts = cuts;
        cuts.push(0);
        cuts.push(STREAM.len());
        cuts.sort_unstable();
        let mut parser = SseParser::new(4096);
        let mut actual = Vec::new();
        for pair in cuts.windows(2) {
            actual.extend(parser.push(&STREAM.as_bytes()[pair[0]..pair[1]]).unwrap());
        }
        actual.extend(parser.finish().unwrap());
        prop_assert_eq!(actual, expected);
    }
}
