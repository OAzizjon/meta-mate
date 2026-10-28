//! Intent routing as pure data, with an offline fallback.

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Where,
    How,
    Chat,
    Do,
}

const CLASSIFIER: &str = "Classify the user's request. Reply with one word:\n\
WHERE  - they want to find one thing on the screen\n\
HOW    - they want to do a task that needs several steps\n\
CHAT   - conversation, a story, a joke, a question not about the screen\n\
DO     - they ask you to do it for them\n";

/// A non-streamed Responses API body; send it with proxy call kind `intent`.
pub fn intent_request(question: &str) -> Value {
    json!({
        "model": "muse-spark-1.3",
        "reasoning": {"effort": "minimal"},
        "max_output_tokens": 200,
        "stream": false,
        "instructions": CLASSIFIER,
        "input": [{"type": "message", "role": "user", "content": [
            {"type": "input_text", "text": question}
        ]}]
    })
}

/// Read the first word, ignoring surrounding punctuation and Markdown. Ambiguous
/// responses containing another label are rejected, even when it repeats the first.
pub fn parse_intent(text: &str) -> Option<Intent> {
    let lower = text.to_lowercase();
    let mut words = words(&lower);
    let intent = label(words.next()?)?;
    if words.any(|word| label(word).is_some()) {
        return None;
    }
    Some(intent)
}

fn label(word: &str) -> Option<Intent> {
    match word {
        "where" => Some(Intent::Where),
        "how" => Some(Intent::How),
        "chat" => Some(Intent::Chat),
        "do" => Some(Intent::Do),
        _ => None,
    }
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|word| !word.is_empty())
}

fn has_phrase(words: &[&str], phrase: &[&str]) -> bool {
    words.windows(phrase.len()).any(|window| window == phrase)
}

fn spoken_words(text: &str) -> String {
    text.to_lowercase().replace(['\'', '’'], "")
}

fn contains_spoken(words: &[&str], phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| {
        let phrase: Vec<_> = phrase.split_whitespace().collect();
        has_phrase(words, &phrase)
    })
}

fn has_no(words: &[&str]) -> bool {
    contains_spoken(
        words,
        &[
            "no",
            "nope",
            "dont",
            "do not",
            "stop",
            "cancel",
            "wait",
            "нет",
            "не надо",
            "не нажимай",
            "не нажимайте",
            "стоп",
            "отмена",
            "подожди",
            "подождите",
        ],
    )
}

/// Spoken confirmation for the current offer only; a refusal always wins.
pub fn parse_yes_no(text: &str) -> Option<bool> {
    let lower = spoken_words(text);
    let words: Vec<_> = words(&lower).collect();
    let yes = contains_spoken(
        &words,
        &[
            "yes",
            "yeah",
            "yep",
            "sure",
            "ok",
            "okay",
            "please do",
            "go ahead",
            "do it",
            "press it",
            "да",
            "ага",
            "угу",
            "конечно",
            "давай",
            "давайте",
            "нажми",
            "нажмите",
            "хорошо",
            "ладно",
        ],
    );
    if has_no(&words) || (yes && negated(&words)) {
        Some(false)
    } else if doubtful(&words) && !pressing(&words) {
        // "да вряд ли", "yes maybe later": not a yes Mate may press on; he asks again.
        None
    } else if yes {
        Some(true)
    } else {
        None
    }
}

/// A negation anywhere ("не", "ни", "not", "never"): a yes with one is no yes.
fn negated(words: &[&str]) -> bool {
    words
        .iter()
        .any(|word| matches!(*word, "not" | "never" | "не" | "ни" | "никогда"))
}

/// An explicit "press it" ("да ладно, нажми"): it outweighs a hesitant opening.
fn pressing(words: &[&str]) -> bool {
    contains_spoken(
        words,
        &[
            "press it",
            "do it",
            "go ahead",
            "нажми",
            "нажмите",
            "давай",
            "давайте",
            "сделай",
            "сделайте",
        ],
    )
}

/// Hesitation or a "later": not consent to press now.
fn doubtful(words: &[&str]) -> bool {
    contains_spoken(
        words,
        &[
            "maybe",
            "perhaps",
            "later",
            "not now",
            "i dont know",
            "not sure",
            "hardly",
            "вряд ли",
            "может быть",
            "наверное",
            "потом",
            "позже",
            "не знаю",
            "не уверен",
            "не уверена",
            "брось",
            "да ладно",
        ],
    )
}

/// Recognize explicit delegation, without confusing a how-to question with consent.
pub fn asks_do_it(text: &str) -> bool {
    let lower = spoken_words(text);
    let words: Vec<_> = words(&lower).collect();
    !has_no(&words)
        && !negated(&words)
        && contains_spoken(
            &words,
            &[
                "do it for me",
                "press it for me",
                "can you press it",
                "click it for me",
                "can you click it",
                "could you press it",
                "could you click it",
                "сделай за меня",
                "сделайте за меня",
                "нажми за меня",
                "нажмите за меня",
                "нажмите сами",
                "нажми сам",
                "сделайте сами",
                "сделай сам",
                "можешь нажать",
                "можете нажать",
            ],
        )
}

/// Conservative English/Russian fallback when the classifier is unavailable.
/// Delegation wins over finding, which wins over teaching when cues overlap.
pub fn guess_intent(question: &str) -> Intent {
    let lower = question.to_lowercase();
    let words: Vec<&str> = words(&lower).collect();
    if [
        &["do", "it"][..],
        &["for", "me"][..],
        &["сделай", "за", "меня"][..],
        &["нажми", "за", "меня"][..],
    ]
    .iter()
    .any(|phrase| has_phrase(&words, phrase))
    {
        return Intent::Do;
    }
    if words
        .iter()
        .any(|word| matches!(*word, "where" | "find" | "где" | "найди" | "покажи"))
        || has_phrase(&words, &["show", "me"])
    {
        return Intent::Where;
    }
    if has_phrase(&words, &["how", "do", "i"])
        || has_phrase(&words, &["how", "to"])
        || words.contains(&"научи")
        || words.iter().enumerate().any(|(index, word)| {
            if *word != "как" {
                return false;
            }
            // An infinitive after optional pronouns/modal words avoids treating
            // greetings such as "как дела" as instructions.
            words[index + 1..]
                .iter()
                .find(|word| {
                    !matches!(
                        **word,
                        "мне"
                            | "нам"
                            | "можно"
                            | "я"
                            | "мы"
                            | "могу"
                            | "можем"
                            | "мог"
                            | "бы"
                            | "нужно"
                            | "надо"
                    )
                })
                .is_some_and(|word| {
                    let verb = word.strip_suffix("ся").or_else(|| word.strip_suffix("сь"));
                    let verb = verb.unwrap_or(word);
                    verb.ends_with("ть") || verb.ends_with("ти") || verb.ends_with("чь")
                })
        })
    {
        return Intent::How;
    }
    Intent::Chat
}
