use mm_core::geom::{PhysPoint, PhysRect};
use mm_core::redaction::{
    Field, RedactionError, RedactionInput, overlaps_any, painted_areas, point_in_any, redact,
    same_areas,
};
use mm_core::shot::Bgra;

const W: u32 = 40;
const H: u32 = 20;

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PhysRect {
    PhysRect {
        left,
        top,
        right,
        bottom,
    }
}

#[test]
fn painted_areas_leaves_a_fully_covered_region_clear() {
    let region = rect(-10, -10, 10, 10);
    assert!(painted_areas(region, region, &[], &[]).is_empty());
    assert!(painted_areas(region, rect(-10, -10, 0, 10), &[rect(0, -10, 10, 10)], &[]).is_empty());
}

#[test]
fn painted_areas_covers_every_gap_around_a_popup_sticking_out() {
    let region = rect(-5, -5, 15, 15);
    let window = rect(-5, -5, 5, 5);
    let popup = rect(2, 2, 15, 10);
    let painted = painted_areas(region, window, &[popup], &[]);
    for y in region.top..region.bottom {
        for x in region.left..region.right {
            let point = PhysPoint { x, y };
            assert_eq!(
                point_in_any(point, &painted),
                !window.contains(point) && !popup.contains(point),
                "pixel ({x}, {y})"
            );
        }
    }
    assert!(painted.iter().all(|r| r.left < r.right && r.top < r.bottom));
}

#[test]
fn painted_areas_clips_occluders_and_skips_those_outside() {
    let region = rect(0, 0, 10, 10);
    assert_eq!(
        painted_areas(
            region,
            region,
            &[],
            &[rect(-5, 2, 5, 8), rect(10, 0, 20, 10)]
        ),
        vec![rect(0, 2, 5, 8)]
    );
    assert!(painted_areas(region, region, &[], &[rect(20, 20, 30, 30)]).is_empty());
}

#[test]
fn painted_areas_skips_empty_and_inverted_rectangles() {
    let region = rect(0, 0, 10, 10);
    let invalid = [rect(8, 0, 2, 10), rect(0, 8, 10, 2), rect(0, 0, 0, 10)];
    for bad in invalid {
        assert!(painted_areas(bad, region, &[region], &[region]).is_empty());
        assert_eq!(painted_areas(region, bad, &invalid, &invalid), vec![region]);
    }
}

#[test]
fn painted_hits_use_half_open_bounds_and_ignore_invalid_rectangles() {
    let painted = [rect(0, 0, 10, 10), rect(30, 30, 20, 20)];
    assert!(overlaps_any(rect(-1, -1, 1, 1), &painted));
    assert!(overlaps_any(rect(2, 2, 8, 8), &painted));
    for clear in [
        rect(10, 0, 20, 10),
        rect(0, 10, 10, 20),
        rect(5, 5, 5, 8),
        rect(8, 8, 2, 2),
    ] {
        assert!(!overlaps_any(clear, &painted));
    }
    assert!(point_in_any(PhysPoint { x: 0, y: 0 }, &painted));
    assert!(point_in_any(PhysPoint { x: 9, y: 9 }, &painted));
    for point in [
        PhysPoint { x: 10, y: 5 },
        PhysPoint { x: 5, y: 10 },
        PhysPoint { x: -1, y: 0 },
        PhysPoint { x: 25, y: 25 },
    ] {
        assert!(!point_in_any(point, &painted));
    }
    assert!(!overlaps_any(rect(0, 0, 10, 10), &[]));
    assert!(!point_in_any(PhysPoint { x: 0, y: 0 }, &[]));
}

#[test]
fn same_areas_ignores_order_but_not_a_window_that_came_went_or_moved() {
    let (a, b) = (rect(0, 0, 10, 10), rect(5, 5, 20, 20));
    assert!(same_areas(&[], &[]));
    assert!(same_areas(&[a, b], &[b, a]));
    assert!(!same_areas(&[a], &[a, b]));
    assert!(!same_areas(&[a, b], &[a]));
    assert!(!same_areas(&[a], &[rect(1, 0, 11, 10)]));
    assert!(!same_areas(&[a, a], &[a]));
}

fn white() -> Vec<u8> {
    vec![255; (W * H * 4) as usize]
}

fn region() -> PhysRect {
    // A window on a monitor left of the primary one.
    PhysRect::new(-100, 50, -100 + W as i32, 50 + H as i32).unwrap()
}

fn field(password_like: bool) -> Field {
    Field {
        // Physical: x -90..-80, y 55..60 → capture pixels x 10..20, y 5..10.
        rect: PhysRect::new(-90, 55, -80, 60).unwrap(),
        is_password: false,
        password_like,
        was_password_this_session: false,
    }
}

fn input<'a>(pixels: &'a [u8], fields: &'a [Field]) -> RedactionInput<'a> {
    RedactionInput {
        capture: Bgra::new(W, H, pixels).unwrap(),
        region: region(),
        snapshot_region: region(),
        fields,
        snapshot_truncated: false,
        unclassified_sensitive_field: false,
        max_edge: 0,
    }
}

fn decode(data_url: &str) -> Vec<u8> {
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD
        .decode(data_url.trim_start_matches("data:image/png;base64,"))
        .unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    let mut rgb = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut rgb).unwrap();
    rgb
}

#[test]
fn a_password_like_field_is_painted_and_ordinary_ones_are_not() {
    let pixels = white();
    let fields = [field(true), field(false)];
    let redacted = redact(&input(&pixels, &fields)).unwrap();
    assert_eq!(redacted.painted, 1);
    let rgb = decode(&debug_data_url(&redacted));
    let at = |x: u32, y: u32| rgb[((y * W + x) * 3) as usize];
    assert_eq!(at(15, 7), 0, "inside the field");
    assert_eq!(at(7, 7), 0, "inside the 4 px padding");
    assert_eq!(at(30, 15), 255, "elsewhere");
    assert_eq!(redacted.frame.region(), region());
}

/// The data URL is private to the request builders; tests read it back through the
/// request body, as the network client would send it.
fn debug_data_url(redacted: &mm_core::redaction::Redacted) -> String {
    let list = mm_core::api::request::render_element_list(&[], 10);
    let body = mm_core::api::request::point_request(
        Default::default(),
        mm_core::api::request::ReasoningEffort::SPOKEN,
        "Where?",
        &list,
        &redacted.image,
    );
    body["input"][0]["content"][1]["image_url"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn refuses_when_redaction_cannot_be_guaranteed() {
    let pixels = white();
    let fields = [field(true)];
    let mut truncated = input(&pixels, &fields);
    truncated.snapshot_truncated = true;
    assert_eq!(
        redact(&truncated).unwrap_err(),
        RedactionError::SnapshotTruncated
    );
    let mut unclear = input(&pixels, &fields);
    unclear.unclassified_sensitive_field = true;
    assert_eq!(
        redact(&unclear).unwrap_err(),
        RedactionError::UnclassifiedSensitiveField
    );
    // The capture is 40x20 but both the snapshot and the capture claim 41x20.
    let mut resized = input(&pixels, &fields);
    resized.region = PhysRect::new(0, 0, 41, 20).unwrap();
    resized.snapshot_region = resized.region;
    assert_eq!(
        redact(&resized).unwrap_err(),
        RedactionError::CaptureMismatch
    );
}

#[test]
fn downscaling_keeps_the_frame_transform_of_the_sent_image() {
    let pixels = white();
    let mut half = input(&pixels, &[]);
    half.max_edge = 20;
    let redacted = redact(&half).unwrap();
    assert_eq!(redacted.painted, 0);
    let frame = redacted.frame;
    // The model's pixel (10, 5) in the 20x10 image is physical (-80, 60).
    let phys = frame.shot_to_phys(mm_core::geom::ShotPoint { x: 10.0, y: 5.0 });
    assert_eq!((phys.x, phys.y), (-80, 60));
}

#[test]
fn rgb_redaction_matches_painting_bgra_before_downscaling() {
    use mm_core::shot::{ShotRectPx, downscale_to_rgb, paint_black};

    let pixels: Vec<u8> = (0..W as usize * H as usize * 4)
        .map(|i| (i.wrapping_mul(37) ^ (i / 7)) as u8)
        .collect();
    let fields = [field(true)];
    let mut painted = pixels.clone();
    paint_black(
        &mut painted,
        W,
        H,
        &[ShotRectPx {
            left: 10,
            top: 5,
            right: 20,
            bottom: 10,
        }],
    )
    .unwrap();
    for edge in [0, 1, 7, 20, 39, 40] {
        let mut request = input(&pixels, &fields);
        request.max_edge = edge;
        let redacted = redact(&request).unwrap();
        let (expected, _, _) = downscale_to_rgb(Bgra::new(W, H, &painted).unwrap(), edge).unwrap();
        assert_eq!(decode(&debug_data_url(&redacted)), expected);
        assert_eq!(redacted.painted, 1);
    }
    assert!(
        pixels
            .iter()
            .enumerate()
            .all(|(i, value)| { *value == (i.wrapping_mul(37) ^ (i / 7)) as u8 }),
        "the borrowed source must stay unchanged"
    );
}

#[test]
fn a_window_that_moved_after_the_snapshot_is_refused() {
    let pixels = white();
    let fields = [field(true)];
    let mut moved = input(&pixels, &fields);
    // Same size, new place: the old field rects would paint the wrong pixels.
    moved.region = PhysRect::new(0, 0, W as i32, H as i32).unwrap();
    assert_eq!(redact(&moved).unwrap_err(), RedactionError::StaleSnapshot);
}

#[test]
fn a_hidden_field_with_invalid_bounds_is_refused_not_skipped() {
    let pixels = white();
    let mut broken = field(true);
    broken.rect = PhysRect {
        left: -80,
        top: 55,
        right: -90,
        bottom: 60,
    };
    let fields = [broken];
    assert_eq!(
        redact(&input(&pixels, &fields)).unwrap_err(),
        RedactionError::InvalidField
    );
    // An ordinary field with odd bounds does not block the screenshot.
    let mut plain = broken;
    plain.password_like = false;
    let fields = [plain];
    assert!(redact(&input(&pixels, &fields)).is_ok());
}
