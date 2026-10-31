//! Build the app's safety request for a synthetic benchmark screenshot (no live capture).
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

use mm_core::api::request::{ModelChoice, ReasoningEffort, render_element_list, safety_request};
use mm_core::geom::PhysRect;
use mm_core::redaction::{RedactionInput, redact};
use mm_core::shot::Bgra;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    println!("{}", serde_json::to_string(&build_request(&args)?)?);
    Ok(())
}

pub(crate) fn build_request(args: &[String]) -> Result<serde_json::Value, Box<dyn Error>> {
    if !args.len().is_multiple_of(2) {
        return Err(
            "expected --image PNG --lang en|ru [--max-edge PX] [--model ID] [--reasoning EFFORT]"
                .into(),
        );
    }
    for pair in args.as_chunks::<2>().0 {
        if !["--image", "--lang", "--max-edge", "--model", "--reasoning"]
            .contains(&pair[0].as_str())
        {
            return Err("unknown argument".into());
        }
    }
    let value = |name: &str| {
        args.as_chunks::<2>()
            .0
            .iter()
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].as_str())
    };
    let image = value("--image").ok_or("--image is required")?;
    let lang = value("--lang").ok_or("--lang is required")?;
    let locale = match lang {
        "en" => include_str!("../../../locales/en.json"),
        "ru" => include_str!("../../../locales/ru.json"),
        _ => return Err("language must be en or ru".into()),
    };
    let locale: serde_json::Value = serde_json::from_str(locale)?;
    let question = locale["tray.try_safe"]
        .as_str()
        .ok_or("missing safety question")?;
    let model = match value("--model").unwrap_or("muse-spark-1.3") {
        "muse-spark-1.3" => ModelChoice::Quick,
        "muse-wonder-1.0" => ModelChoice::Thorough,
        _ => return Err("unsupported safety model".into()),
    };
    let effort = match value("--reasoning") {
        None => ReasoningEffort::SAFETY_CHECK,
        Some("minimal") => ReasoningEffort::Minimal,
        Some("low") => ReasoningEffort::Low,
        Some("medium") => ReasoningEffort::Medium,
        Some("high") => ReasoningEffort::High,
        _ => return Err("unsupported reasoning effort".into()),
    };
    let max_edge = value("--max-edge").unwrap_or("1280").parse()?;
    let mut decoder = png::Decoder::new(BufReader::new(File::open(image)?));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut bytes = vec![0; reader.output_buffer_size().ok_or("PNG too large")?];
    let info = reader.next_frame(&mut bytes)?;
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        _ => return Err("unsupported PNG color type".into()),
    };
    let mut bgra = Vec::new();
    for pixel in bytes[..info.buffer_size()].chunks_exact(channels) {
        if channels < 3 {
            bgra.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]);
        } else {
            bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
    }
    let region = PhysRect::new(0, 0, info.width.try_into()?, info.height.try_into()?)?;
    // These are synthetic fixtures, with no personal data or live UIA snapshot.
    let redacted = redact(&RedactionInput {
        capture: Bgra::new(info.width, info.height, &bgra)?,
        region,
        snapshot_region: region,
        fields: &[],
        snapshot_truncated: false,
        unclassified_sensitive_field: false,
        max_edge,
    })?;
    Ok(safety_request(
        model,
        effort,
        question,
        &render_element_list(&[], 0),
        &redacted.image,
    ))
}
