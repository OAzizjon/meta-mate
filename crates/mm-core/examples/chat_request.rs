//! Build the app's chat request for prompt tuning (no screen), or parse a MateScript
//! answer with the app's parser. `mm probe chat` (tools/mm_probe_chat.py) runs this
//! example like the safety probe runs safety_request: the JSON body is printed to stdout
//! and sent as is, so the probe exercises the real request builder. `--parse TEXT`
//! prints the parsed items (sentences, <show>, <open>, moods, sounds) as JSON instead.
use std::error::Error;

use mm_core::api::profile::with_profile;
use mm_core::api::request::{ModelChoice, ReasoningEffort, chat_request};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--parse") {
        let [_, text] = args.as_slice() else {
            return Err("expected --parse TEXT".into());
        };
        // The app's parser, as the chat path sets it up (language from the answer's <lang>).
        let mut parser = mm_core::matescript::Parser::new(mm_core::matescript::Settings::default());
        let mut items = parser.push(text);
        items.extend(parser.finish());
        println!("{}", serde_json::to_string(&items)?);
        return Ok(());
    }
    println!("{}", serde_json::to_string(&build_request(&args)?)?);
    Ok(())
}

pub(crate) fn build_request(args: &[String]) -> Result<serde_json::Value, Box<dyn Error>> {
    if !args.len().is_multiple_of(2) {
        return Err(
            "expected --question TEXT [--lang en|ru] [--name NAME] [--about TEXT] [--model ID] [--reasoning EFFORT]"
                .into(),
        );
    }
    for pair in args.as_chunks::<2>().0 {
        if ![
            "--question",
            "--lang",
            "--name",
            "--about",
            "--model",
            "--reasoning",
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
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].as_str())
    };
    let question = value("--question").ok_or("--question is required")?;
    let model = match value("--model").unwrap_or("muse-spark-1.3") {
        "muse-spark-1.3" => ModelChoice::Quick,
        "muse-wonder-1.0" => ModelChoice::Thorough,
        _ => return Err("unsupported chat model".into()),
    };
    let effort = match value("--reasoning") {
        None => ReasoningEffort::SPOKEN,
        Some("minimal") => ReasoningEffort::Minimal,
        Some("low") => ReasoningEffort::Low,
        Some("medium") => ReasoningEffort::Medium,
        Some("high") => ReasoningEffort::High,
        _ => return Err("unsupported reasoning effort".into()),
    };
    let lang = match value("--lang") {
        Some("en") | Some("ru") => value("--lang"),
        Some(_) => return Err("language must be en or ru".into()),
        None => None,
    };
    let body = chat_request(model, effort, lang, question);
    // THE PERSON goes in exactly as the app adds it (voice.rs: with_profile on every answer).
    let profile = mm_core::api::profile::Profile {
        name: value("--name").unwrap_or_default().to_owned(),
        about: value("--about").unwrap_or_default().to_owned(),
    };
    Ok(with_profile(body, &profile))
}
