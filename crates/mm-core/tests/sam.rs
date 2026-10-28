use mm_core::api::sam::{SamBox, parse_sam_boxes, pick_sam_box, sam_point_request};
use mm_core::geom::PhysRect;
use mm_core::redaction::{RedactionInput, redact};
use mm_core::shot::Bgra;
use serde_json::{Value, json};

macro_rules! replay {
    ($path:literal) => {
        include_str!(concat!("../../../fixtures/replay/", $path))
    };
}

const SENT: (u32, u32) = (1280, 800);

const SEARCH_ANSWER: &str =
    "<0f>0<|box;x1=849;y1=20;x2=940;y2=56;w=1280;h=800|><|mask;x=0;y=0;data=...|>";
const RADIO_ANSWER: &str = "<0f>1<|box;x1=960;y1=262;x2=979;y2=279;w=1280;h=800|><|mask;x=0;y=0;data=18,20,!!!!!=Ibc#]ML_U=MfppNL^#pBwK1yXpg$qj|>\n";

const SEARCH_PROBE: &str = replay!("synthetic-text-size/probe-20261027T191409520950Z-meta.json");
const RADIO_PROBE: &str = replay!("synthetic-text-size/probe-20261027T191413291062Z-meta.json");

fn test_image() -> (mm_core::redaction::RedactedImage, String) {
    let pixels = vec![255u8; 4 * 4 * 4];
    let region = PhysRect::new(0, 0, 4, 4).unwrap();
    let redacted = redact(&RedactionInput {
        capture: Bgra::new(4, 4, &pixels).unwrap(),
        region,
        snapshot_region: region,
        fields: &[],
        snapshot_truncated: false,
        unclassified_sensitive_field: false,
        max_edge: 0,
    })
    .unwrap();
    let list = mm_core::api::request::render_element_list(&[], 10);
    let url = mm_core::api::request::point_request(
        Default::default(),
        mm_core::api::request::ReasoningEffort::SPOKEN,
        "Where?",
        &list,
        &redacted.image,
    )["input"][0]["content"][1]["image_url"]
        .as_str()
        .unwrap()
        .to_owned();
    (redacted.image, url)
}

#[test]
fn point_request_matches_the_measured_body_and_clamps_coordinates() {
    let (image, url) = test_image();
    let body = sam_point_request(895, 38, SENT, &image);
    assert_eq!(
        body,
        json!({
            "model": "sam-3.1",
            "stream": true,
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "<0f>1<|point;x=895;y=38;w=1280;h=800|>"
                    },
                    {
                        "type": "input_image",
                        "image_url": url
                    }
                ]
            }]
        })
    );

    let clamped = sam_point_request(5000, 800, SENT, &image);
    assert_eq!(
        clamped["input"][0]["content"][0]["text"],
        "<0f>1<|point;x=1279;y=799;w=1280;h=800|>"
    );

    let zero_size = sam_point_request(10, 20, (0, 0), &image);
    assert_eq!(
        zero_size["input"][0]["content"][0]["text"],
        "<0f>1<|point;x=0;y=0;w=0;h=0|>"
    );
}

#[test]
fn measured_answers_parse_and_pick_the_search_and_radio_buttons() {
    let search = SamBox {
        left: 849,
        top: 20,
        right: 941,
        bottom: 57,
    };
    assert_eq!(search.width(), 92);
    assert_eq!(search.height(), 37);
    let search_boxes = parse_sam_boxes(SEARCH_ANSWER, SENT);
    assert_eq!(search_boxes, [search]);
    assert_eq!(pick_sam_box(&search_boxes, 895, 38, SENT), Some(search));

    let radio = SamBox {
        left: 960,
        top: 262,
        right: 980,
        bottom: 280,
    };
    assert_eq!(radio.width(), 20);
    assert_eq!(radio.height(), 18);
    let radio_boxes = parse_sam_boxes(RADIO_ANSWER, SENT);
    assert_eq!(radio_boxes, [radio]);
    assert_eq!(pick_sam_box(&radio_boxes, 975, 271, SENT), Some(radio));

    for (probe, x, y, expected) in [
        (SEARCH_PROBE, 895, 38, search),
        (RADIO_PROBE, 975, 271, radio),
    ] {
        let recorded: Value = serde_json::from_str(probe).unwrap();
        let head = recorded["answer_head"].as_str().unwrap();
        let boxes = parse_sam_boxes(head, SENT);
        assert_eq!(boxes, [expected]);
        assert_eq!(pick_sam_box(&boxes, x, y, SENT), Some(expected));
    }
}

#[test]
fn parsing_is_independent_of_how_stream_chunks_were_accumulated() {
    let full = format!("{SEARCH_ANSWER},{RADIO_ANSWER}");
    let expected = parse_sam_boxes(&full, SENT);
    assert_eq!(expected.len(), 2);

    for step in [1usize, 2, 3, 5, 7, 13, 29, 64] {
        let mut accumulated = String::new();
        for chunk in full.as_bytes().chunks(step) {
            accumulated.push_str(std::str::from_utf8(chunk).unwrap());
        }
        let boxes = parse_sam_boxes(&accumulated, SENT);
        assert_eq!(boxes, expected, "chunk size {step}");
        assert_eq!(
            pick_sam_box(&boxes, 895, 38, SENT),
            pick_sam_box(&expected, 895, 38, SENT)
        );
        assert_eq!(
            pick_sam_box(&boxes, 975, 271, SENT),
            pick_sam_box(&expected, 975, 271, SENT)
        );
    }
}

#[test]
fn boxes_with_wrong_width_or_height_are_skipped() {
    let text = concat!(
        "<0f>0<|box;x1=10;y1=10;x2=50;y2=50;w=1279;h=800|><|mask;x=0;y=0;data=abc|>,",
        "<|box;x1=10;y1=10;x2=50;y2=50;w=1280;h=801|><|mask;x=0;y=0;data=def|>,",
        "<|box;x1=10;y1=10;x2=50;y2=50;w=640;h=400|>,",
        "<|box;x1=20;y1=30;x2=60;y2=70;w=1280;h=800|>"
    );
    assert_eq!(
        parse_sam_boxes(text, SENT),
        [SamBox {
            left: 20,
            top: 30,
            right: 61,
            bottom: 71,
        }]
    );
    assert!(parse_sam_boxes(text, (0, 0)).is_empty());
}

#[test]
fn inclusive_wire_edges_convert_to_half_open_bounds() {
    // x2 = w - 1 and y2 = h - 1 are the last valid pixels; x2 = w or y2 = h are out of bounds.
    let text = concat!(
        "<|box;x1=0;y1=0;x2=1279;y2=799;w=1280;h=800|>,",
        "<|box;x1=1272;y1=792;x2=1279;y2=799;w=1280;h=800|>,",
        "<|box;x1=0;y1=0;x2=1280;y2=799;w=1280;h=800|>,",
        "<|box;x1=0;y1=0;x2=1279;y2=800;w=1280;h=800|>,",
        "<|box;x1=50;y1=20;x2=49;y2=30;w=1280;h=800|>,",
        "<|box;x1=20;y1=50;x2=30;y2=49;w=1280;h=800|>,",
        "<|box;x1=100;y1=200;x2=100;y2=200;w=1280;h=800|>"
    );
    let boxes = parse_sam_boxes(text, SENT);
    let corner = SamBox {
        left: 1272,
        top: 792,
        right: 1280,
        bottom: 800,
    };
    assert_eq!(
        boxes,
        [
            SamBox {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 800,
            },
            corner,
            SamBox {
                left: 100,
                top: 200,
                right: 101,
                bottom: 201,
            },
        ]
    );
    // Both x1/y1 (1272, 792) and inclusive x2/y2 (1279, 799) lie inside the 8x8 corner box.
    assert_eq!(pick_sam_box(&[corner], 1272, 792, SENT), Some(corner));
    assert_eq!(pick_sam_box(&[corner], 1279, 799, SENT), Some(corner));
    // Half-open right/bottom (x2 + 1, y2 + 1) lie outside.
    let inner = SamBox {
        left: 10,
        top: 20,
        right: 30,
        bottom: 40,
    };
    assert_eq!(pick_sam_box(&[inner], 10, 20, SENT), Some(inner));
    assert_eq!(pick_sam_box(&[inner], 29, 39, SENT), Some(inner));
    assert_eq!(pick_sam_box(&[inner], 30, 25, SENT), None);
    assert_eq!(pick_sam_box(&[inner], 15, 40, SENT), None);
}

#[test]
fn huge_boxes_over_forty_percent_of_the_image_are_rejected() {
    let sent = (100, 100);
    // Exactly 40% (40 * 100 = 4_000 of 10_000 px) is accepted.
    let exact = SamBox {
        left: 0,
        top: 0,
        right: 40,
        bottom: 100,
    };
    assert_eq!(pick_sam_box(&[exact], 20, 50, sent), Some(exact));

    // 41% (41 * 100 = 4_100 of 10_000 px) and full-window boxes are rejected.
    let over = SamBox {
        left: 0,
        top: 0,
        right: 41,
        bottom: 100,
    };
    let full = SamBox {
        left: 0,
        top: 0,
        right: 100,
        bottom: 100,
    };
    assert_eq!(pick_sam_box(&[over, full], 20, 50, sent), None);
}

#[test]
fn tiny_boxes_under_eight_by_eight_are_rejected() {
    let b8 = SamBox {
        left: 10,
        top: 10,
        right: 18,
        bottom: 18,
    };
    let w7 = SamBox {
        left: 10,
        top: 10,
        right: 17,
        bottom: 20,
    };
    let h7 = SamBox {
        left: 10,
        top: 10,
        right: 20,
        bottom: 17,
    };
    let px1 = SamBox {
        left: 12,
        top: 12,
        right: 13,
        bottom: 13,
    };
    assert_eq!(pick_sam_box(&[w7, h7, px1], 12, 12, SENT), None);
    assert_eq!(pick_sam_box(&[w7, h7, px1, b8], 12, 12, SENT), Some(b8));
}

#[test]
fn points_outside_the_box_or_image_return_none() {
    let button = SamBox {
        left: 849,
        top: 20,
        right: 941,
        bottom: 57,
    };
    for (x, y) in [
        (848, 38),
        (941, 38),
        (895, 19),
        (895, 57),
        (0, 0),
        (1280, 38),
        (895, 800),
    ] {
        assert_eq!(pick_sam_box(&[button], x, y, SENT), None, "({x}, {y})");
    }
}

#[test]
fn several_boxes_pick_the_smallest_valid_one_containing_the_point() {
    let text = concat!(
        // Huge panel (800x600 = 480_000 > 40% of 1_024_000 -> 409_600): rejected.
        "<0f>0<|box;x1=100;y1=100;x2=899;y2=699;w=1280;h=800|><|mask;x=0;y=0;data=AAA|>,",
        // Toolbar (300x60 = 18_000): valid, contains (215, 215).
        "<|box;x1=150;y1=190;x2=449;y2=249;w=1280;h=800|><|mask;x=0;y=0;data=BBB|>,",
        // Icon button inside toolbar (24x24 = 576): smallest valid, contains (215, 215).
        "<|box;x1=200;y1=200;x2=223;y2=223;w=1280;h=800|><|mask;x=0;y=0;data=CCC|>,",
        // Tiny glyph inside icon (6x6): rejected (< 8x8).
        "<|box;x1=212;y1=212;x2=217;y2=217;w=1280;h=800|><|mask;x=0;y=0;data=DDD|>,",
        // Small button elsewhere (16x16): does not contain (215, 215).
        "<|box;x1=500;y1=500;x2=515;y2=515;w=1280;h=800|><|mask;x=0;y=0;data=EEE|>"
    );
    let boxes = parse_sam_boxes(text, SENT);
    assert_eq!(boxes.len(), 5);
    assert_eq!(
        pick_sam_box(&boxes, 215, 215, SENT),
        Some(SamBox {
            left: 200,
            top: 200,
            right: 224,
            bottom: 224,
        })
    );
}

#[test]
fn empty_text_and_garbage_are_ignored_safely() {
    assert!(parse_sam_boxes("", SENT).is_empty());
    assert_eq!(pick_sam_box(&[], 100, 100, SENT), None);

    for garbage in [
        "no tags here",
        "<0f>0",
        "<|box;x1=10;y1=10;x2=20;y2=20;w=1280;h=800",
        "<|box;x1=a;y1=10;x2=20;y2=20;w=1280;h=800|>",
        "<|box;x1=-1;y1=10;x2=20;y2=20;w=1280;h=800|>",
        "<|box;x1=+1;y1=10;x2=20;y2=20;w=1280;h=800|>",
        "<|box;x1=10;y1=10;x2=20;y2=20;w=1280|>",
        "<|box;x1=10;y1=10;x2=20;y2=20;w=1280;h=800;extra=1|>",
        "<|box;y1=10;x1=10;x2=20;y2=20;w=1280;h=800|>",
        "<|box;x1=99999999999999999999;y1=10;x2=20;y2=20;w=1280;h=800|>",
        "<|mask;x=0;y=0;data=37,92,!!!!!CLByL98=i-c6q[]{soN+_&U6e!DIOsJ5Whg%!!|>",
    ] {
        assert!(parse_sam_boxes(garbage, SENT).is_empty(), "{garbage}");
    }

    // A truncated `<|box;` before a valid `<|box;...|>` does not swallow the valid box.
    let recovered = "<|box;x1=10;y1=20<|box;x1=849;y1=20;x2=940;y2=56;w=1280;h=800|><|box;broken";
    assert_eq!(
        parse_sam_boxes(recovered, SENT),
        [SamBox {
            left: 849,
            top: 20,
            right: 941,
            bottom: 57,
        }]
    );
}
