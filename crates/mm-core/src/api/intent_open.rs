//! Whole-app/site requests bypass screen capture; Settings subpages remain guidance.

use crate::matescript::{APPS, OpenTarget};

/// Whether the person's own question asks to open or start something (en/ru verbs, any
/// position): the gate for `<open>` tags (`matescript::Settings::allow_open`).
pub fn wants_open(question: &str) -> bool {
    let lower = question.to_lowercase();
    let words: Vec<&str> = lower
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    // "Don't open it", "не открывай": a negated verb asks for nothing (Meta #1463).
    let negated = |at: usize| {
        words[at.saturating_sub(2)..at].iter().any(|word| {
            matches!(
                *word,
                "not" | "never" | "don" | "dont" | "t" | "no" | "не" | "нельзя" | "никогда"
            )
        })
    };
    words.iter().enumerate().any(|(at, word)| {
        !negated(at)
            && matches!(
                *word,
                "open"
                    | "launch"
                    | "start"
                    | "открой"
                    | "откройте"
                    | "открыть"
                    | "запусти"
                    | "запустите"
                    | "запустить"
            )
    })
}

pub(super) fn asks_open(words: &[&str], raw: &str) -> bool {
    let request: Vec<_> = words
        .iter()
        .copied()
        .filter(|word| {
            !matches!(
                *word,
                "please"
                    | "mate"
                    | "пожалуйста"
                    | "can"
                    | "could"
                    | "you"
                    | "можешь"
                    | "можете"
                    | "ты"
                    | "вы"
                    | "the"
                    | "a"
                    | "мне"
            )
        })
        .collect();
    let Some((verb, target)) = request.split_first() else {
        return false;
    };
    if !matches!(
        *verb,
        "open" | "launch" | "start" | "открой" | "откройте" | "запусти" | "запустите"
    ) {
        return false;
    }
    // Any word may be the address: "open example.com please" (Command Code #1449).
    let address = raw.split_whitespace().any(|last| {
        let last = last.trim_matches(|ch: char| matches!(ch, ',' | '!' | '?' | '"' | '«' | '»'));
        let url = if last.starts_with("https://") {
            last.to_owned()
        } else {
            format!("https://{last}")
        };
        matches!(OpenTarget::parse(&url), Some(OpenTarget::Site(_)))
    });
    address
        || (target.len() == 1 && APPS.iter().any(|app| app.1 == target[0]))
        || matches!(
            target,
            ["youtube"
                | "ютуб"
                | "ютюб"
                | "google"
                | "bing"
                | "yandex"
                | "яндекс"
                | "wikipedia"
                | "википедию"
                | "instagram"
                | "facebook"
                | "netflix"
                | "калькулятор"
                | "блокнот"
                | "настройки"
                | "проводник"
                | "камеру"
                | "фотографии"
                | "часы"
                | "alarms"
                | "будильник"
                | "почту"
                | "календарь"
                | "магазин"]
                | ["file", "explorer"]
                | ["microsoft", "store"]
        )
        || ((matches!(target.first(), Some(&"website" | &"site" | &"сайт"))
            || matches!(target.last(), Some(&"website" | &"site")))
            && !target.iter().any(|word| {
                matches!(
                    *word,
                    "this" | "that" | "here" | "screen" | "этот" | "тот" | "экране"
                )
            }))
}
