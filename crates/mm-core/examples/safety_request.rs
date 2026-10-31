//! Build the app's safety request for a synthetic benchmark screenshot (no live capture).
use std::error::Error;

mod probe;

use mm_core::api::request::{ModelChoice, ReasoningEffort, render_element_list, safety_request};

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
    let redacted = probe::image(image, max_edge)?;
    Ok(safety_request(
        model,
        effort,
        question,
        &render_element_list(&[], 0),
        &redacted.image,
    ))
}
