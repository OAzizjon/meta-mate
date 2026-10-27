use mm_core::geom::PhysRect;
use mm_core::redaction::{Field, RedactionError, RedactionInput, redact};
use mm_core::shot::Bgra;

const W: u32 = 40;
const H: u32 = 20;

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
    let mut moved = input(&pixels, &fields);
    moved.region = PhysRect::new(0, 0, 41, 20).unwrap();
    assert_eq!(redact(&moved).unwrap_err(), RedactionError::CaptureMismatch);
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
