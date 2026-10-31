//! Build the app's pointing request for a synthetic benchmark frame.
use std::error::Error;

use mm_core::api::request::{
    ElementLine, ModelChoice, ReasoningEffort, point_request, render_element_list,
};
use mm_core::geom::PhysRect;
use serde::Deserialize;

mod probe;

#[derive(Deserialize)]
struct Element {
    id: String,
    role: String,
    name: String,
    rect: [i64; 4],
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    println!("{}", build_request(&args)?);
    Ok(())
}

pub(crate) fn build_request(args: &[String]) -> Result<serde_json::Value, Box<dyn Error>> {
    if !args.len().is_multiple_of(2) {
        return Err("expected flag/value pairs".into());
    }
    for pair in args.as_chunks::<2>().0 {
        if ![
            "--image",
            "--question",
            "--model",
            "--reasoning",
            "--max-edge",
            "--elements",
        ]
        .contains(&pair[0].as_str())
        {
            return Err("unknown argument".into());
        }
    }
    let value = |name: &str| {
        args.as_chunks::<2>()
            .0
            .iter()
            .find(|p| p[0] == name)
            .map(|p| p[1].as_str())
    };
    let model = match value("--model").ok_or("--model is required")? {
        "muse-spark-1.3" => ModelChoice::Quick,
        "muse-wonder-1.0" => ModelChoice::Thorough,
        _ => return Err("unsupported pointing model".into()),
    };
    let effort = match value("--reasoning") {
        None => ReasoningEffort::SPOKEN,
        Some("minimal") => ReasoningEffort::Minimal,
        Some("low") => ReasoningEffort::Low,
        Some("medium") => ReasoningEffort::Medium,
        Some("high") => ReasoningEffort::High,
        _ => return Err("unsupported reasoning effort".into()),
    };
    let redacted = probe::image(
        value("--image").ok_or("--image is required")?,
        value("--max-edge").unwrap_or("1280").parse()?,
    )?;
    let elements: Vec<Element> = match value("--elements").unwrap_or("none") {
        "none" => Vec::new(),
        path => serde_json::from_str(&std::fs::read_to_string(path)?)?,
    };
    let mut lines = Vec::new();
    for e in elements {
        let [x, y, w, h] = e.rect;
        let right = x.checked_add(w).ok_or("rectangle overflow")?;
        let bottom = y.checked_add(h).ok_or("rectangle overflow")?;
        let rect = PhysRect::new(
            x.try_into()?,
            y.try_into()?,
            right.try_into()?,
            bottom.try_into()?,
        )?;
        lines.push(
            ElementLine::new(&e.id, &e.role, &e.name, rect, redacted.frame, true, false)
                .ok_or("invalid element id or rectangle")?,
        );
    }
    Ok(point_request(
        model,
        effort,
        value("--question").ok_or("--question is required")?,
        &render_element_list(&lines, 60),
        &redacted.image,
    ))
}
