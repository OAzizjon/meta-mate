//! "Say that again", "slower please", "повтори": the person missed Mate's last answer.
//! Short requests only: "repeat the steps for printing" is a new question.

use super::{contains_spoken, spoken_words, words};

/// At most this many words: longer sentences carry a new question.
const MAX_WORDS: usize = 6;

const PHRASES: &[&str] = &[
    "again",
    "repeat",
    "repeat that",
    "say that again",
    "say it again",
    "one more time",
    "slower",
    "more slowly",
    "slowly please",
    "i didnt hear",
    "i didnt catch that",
    "what did you say",
    "pardon",
    "повтори",
    "повторите",
    "ещё раз",
    "еще раз",
    "помедленнее",
    "медленнее",
    "не расслышал",
    "не расслышала",
    "не понял повтори",
    "что ты сказал",
    "что вы сказали",
];

/// The only words such a request may contain: anything else ("again, where is Save?",
/// "repeat the password", "slower internet") is a new question (Cursor review).
const VOCABULARY: &[&str] = &[
    "again",
    "repeat",
    "that",
    "say",
    "it",
    "one",
    "more",
    "time",
    "slower",
    "slowly",
    "i",
    "didnt",
    "hear",
    "catch",
    "what",
    "did",
    "you",
    "pardon",
    "sorry",
    "can",
    "could",
    "повтори",
    "повторите",
    "ещё",
    "еще",
    "раз",
    "помедленнее",
    "медленнее",
    "не",
    "расслышал",
    "расслышала",
    "понял",
    "поняла",
    "что",
    "ты",
    "вы",
    "сказал",
    "сказали",
    "мне",
    "я",
    "ну",
    "извини",
    "извините",
];

/// Whether `question` only asks Mate to repeat his last answer, slower or simpler.
pub fn asks_repeat(question: &str) -> bool {
    let lower = spoken_words(question);
    let words: Vec<&str> = words(&lower)
        .filter(|word| !matches!(*word, "please" | "mate" | "пожалуйста" | "можно" | "а"))
        .collect();
    !words.is_empty()
        && words.len() <= MAX_WORDS
        && words.iter().all(|word| VOCABULARY.contains(word))
        && contains_spoken(&words, PHRASES)
}

/// The question the model gets instead: the previous answer, said again more simply.
pub fn repeat_question(previous_answer: &str, russian: bool) -> String {
    if russian {
        format!(
            "Человек не расслышал твой прошлый ответ. Скажи его ещё раз, медленнее и проще, короткими предложениями, не добавляя нового. Твой прошлый ответ: «{previous_answer}»"
        )
    } else {
        format!(
            "The person did not catch your last answer. Say it again, slower and simpler, in short sentences, adding nothing new. Your last answer: \"{previous_answer}\""
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_repeat_requests_are_caught_in_both_languages() {
        for question in [
            "Again?",
            "Say that again, please",
            "Slower please",
            "Sorry, what did you say?",
            "I didn't catch that",
            "Повтори",
            "Повторите, пожалуйста",
            "Ещё раз",
            "А можно помедленнее?",
            "Я не расслышала",
        ] {
            assert!(asks_repeat(question), "{question}");
        }
    }

    #[test]
    fn new_questions_and_other_uses_of_again_are_not_repeats() {
        for question in [
            "How do I open my email again after closing it?",
            "Repeat the steps for printing a photo from my phone",
            "Where is the Save button?",
            "Open YouTube",
            "Как снова открыть почту, если я её закрыл?",
            "Again where is Save?",
            "Repeat where is the button?",
            "What did you say about Save?",
            "Slower internet please",
            "Can you repeat the password?",
            "Say it again louder",
            "Ещё раз как сохранить?",
            "Повтори как открыть почту",
            "",
        ] {
            assert!(!asks_repeat(question), "{question}");
        }
    }

    #[test]
    fn the_repeat_question_quotes_the_previous_answer() {
        let english = repeat_question("Click Save.", false);
        assert!(english.contains("\"Click Save.\"") && english.contains("slower"));
        let russian = repeat_question("Нажмите «Сохранить».", true);
        assert!(russian.contains("медленнее") && russian.contains("Сохранить"));
    }
}
