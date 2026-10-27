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
                .find(|word| !matches!(**word, "мне" | "нам" | "можно"))
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
