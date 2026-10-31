//! Intent routing as pure data, with an offline fallback.

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Where,
    How,
    Chat,
    Do,
    /// "Is this safe?": a verdict on the window (scams). Only the offline cues pick it.
    Safety,
}

const CLASSIFIER: &str = "Classify the user's request. Reply with one word:\n\
WHERE  - they want to find one thing on the screen\n\
HOW    - they want guidance through a task, including imperatives like \"click X\", \"press OK\", or \"open Settings\"\n\
CHAT   - conversation, a story, a joke, a question not about the screen\n\
DO     - they ask you to do it for them: \"do it\", \"click it for me\", \"сделай\", or an imperative with \"за меня\", \"сам\", or \"yourself\"\n";

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
    text.to_lowercase().replace(['\'', '’', '‘', 'ʻ', 'ʼ'], "")
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
    parse_yes_no_for_locale(text, "en")
}

/// Uzbek "ha" is affirmative only in the Uzbek locale.
pub fn parse_yes_no_for_locale(text: &str, locale: &str) -> Option<bool> {
    let lower = spoken_words(text);
    let words: Vec<_> = words(&lower).collect();
    if has_no(&words) {
        return Some(false);
    }
    if words.contains(&"ha") && locale != "uz" {
        return None;
    }
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
            "нажми",
            "хорошо",
            "ладно",
            "нажимай",
            "жми",
            "click",
            "ha",
            "mayli",
            "bosing",
        ],
    );
    if yes && negated(&words) {
        Some(false)
    } else if doubtful(&words) && !pressing(&words) {
        // "да вряд ли", "yes maybe later": not a yes Mate may press on; he asks again.
        None
    } else if yes
        && !text.contains('?')
        && words.iter().all(|word| {
            matches!(
                *word,
                "yes"
                    | "yeah"
                    | "yep"
                    | "ok"
                    | "okay"
                    | "sure"
                    | "please"
                    | "go"
                    | "ahead"
                    | "do"
                    | "it"
                    | "press"
                    | "click"
                    | "да"
                    | "давай"
                    | "ага"
                    | "угу"
                    | "конечно"
                    | "пожалуйста"
                    | "нажми"
                    | "нажимай"
                    | "жми"
                    | "хорошо"
                    | "ладно"
                    | "ha"
                    | "mayli"
                    | "bosing"
            )
        })
    {
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

/// How far either side of a verdict its subject may be: "is this message from my bank real".
const VERDICT_REACH: usize = 6;

/// "Is this email real?", "Это мошенники?": a verdict on trust with the thing on the
/// screen (or the person it is aimed at) at most a few words either side of it. A verdict word
/// alone never counts ("Tell me about phone scams"), nor a setting named after one
/// ("safe mode"); "safety" and "безопасности" are no verdicts ("Where is the safety
/// settings?").
fn asks_safety(text: &str) -> bool {
    let lower = spoken_words(text);
    let words: Vec<_> = words(&lower).collect();
    let screen_subject =
        words.iter().any(|word| {
            computer_thing(word)
                || screen_word(word)
                || word.starts_with("нажим")
                || matches!(
                    *word,
                    "message" | "window" | "page" | "link" | "card" | "карта"
                )
        }) || contains_spoken(&words, &["really from", "действительно от", "правда от"]);
    // Bare safety checks keep their screen context; other "it/это" needs a subject.
    let bare_safety = words.iter().enumerate().all(|(at, word)| {
        is_subject(word)
            || is_verdict(&words, at)
            || matches!(
                *word,
                "is" | "are" | "a" | "ли" | "не" | "привет" | "hello" | "mate"
            )
    });
    if (screen_subject
        && (contains_spoken(&words, &["believe it", "это правда"])
            || (words.contains(&"infected") && words.contains(&"computer"))
            || (words.contains(&"карта") && words.iter().any(|w| w.starts_with("заблок")))))
        || (contains_spoken(&words, &["from the bank", "от банка", "из банка"])
            && words.iter().any(|w| matches!(*w, "code" | "код")))
    {
        return true;
    }
    if !bare_safety
        && !screen_subject
        && words.iter().any(|w| matches!(*w, "it" | "это"))
        && !words
            .iter()
            .any(|w| is_subject(w) && !matches!(*w, "it" | "это"))
    {
        return false;
    }
    // Trust in the thing, never in Mate: "Can I trust you?" is a chat.
    let trust = contains_spoken(
        &words,
        &[
            "trust this",
            "trust that",
            "trust it",
            "trust these",
            "trust them",
            "being scammed",
            "getting scammed",
            "really from",
            "доверять этому",
            "доверять этой",
            "доверять этим",
            "доверять им",
            "верить этому",
            "верить этой",
            "действительно от",
            "правда от",
        ],
    );
    trust
        || words.iter().enumerate().any(|(at, _)| {
            is_verdict(&words, at)
                && !words
                    .get(at + 1)
                    .is_some_and(|next| matches!(*next, "mode" | "quick" | "quickly" | "режим"))
                && words
                    [at.saturating_sub(VERDICT_REACH)..(at + VERDICT_REACH + 1).min(words.len())]
                    .iter()
                    .any(|word| is_subject(word))
        })
}

/// The thing on the screen ("this", "это", "тут") or the person a scam is aimed at ("меня").
fn is_subject(word: &str) -> bool {
    matches!(
        word,
        "this"
            | "that"
            | "it"
            | "these"
            | "those"
            | "they"
            | "its"
            | "thats"
            | "theyre"
            | "это"
            | "этот"
            | "эта"
            | "эти"
            | "этому"
            | "этой"
            | "эту"
            | "этом"
            | "этого"
            | "этим"
            | "этих"
            | "тут"
            | "здесь"
            | "он"
            | "она"
            | "оно"
            | "они"
            | "меня"
            | "нас"
            | "мне"
    )
}

/// Russian verdicts inflect ("мошенник", "мошенники", "мошенничество"): matched by stem.
/// "безопасн" is no stem: "безопасности" is the settings page.
// "настоящ" and "развод" need context checks in "is_verdict".
const RUSSIAN_VERDICT_STEMS: [&str; 13] = [
    "мошенн",
    "настоящ",
    "подлинн",
    "поддел",
    "фальшив",
    "фейк",
    "обман",
    "развод",
    "фишинг",
    "афер",
    "надёжн",
    "надежн",
    "опасн",
];

/// A word that judges trust: English words whole, Russian ones by stem or form.
fn is_verdict(words: &[&str], at: usize) -> bool {
    let word = words[at];
    let next = words.get(at + 1).copied();
    // "Real good" is praise; "real email" asks about authenticity.
    if word == "real" {
        return next.is_none_or(|next| {
            matches!(
                next,
                "email"
                    | "message"
                    | "site"
                    | "website"
                    | "page"
                    | "link"
                    | "call"
                    | "letter"
                    | "offer"
            )
        });
    }
    // "Оформить развод" is divorce; "в настоящее время" is a time phrase.
    if word.starts_with("развод")
        && at.checked_sub(1).is_some_and(|before| {
            let previous = words[before];
            matches!(previous, "на" | "о" | "про")
                || previous.starts_with("оформ")
                || previous.starts_with("пода")
        })
        || word.starts_with("настоящ") && next.is_some_and(|next| next.starts_with("врем"))
    {
        return false;
    }
    matches!(
        word,
        "safe"
            | "unsafe"
            | "dangerous"
            | "fake"
            | "legit"
            | "legitimate"
            | "genuine"
            | "authentic"
            | "trustworthy"
            | "suspicious"
            | "scam"
            | "scams"
            | "scammer"
            | "scammers"
            | "scammy"
            | "fraud"
            | "fraudulent"
            | "fraudster"
            | "fraudsters"
            | "phishing"
            | "безопасно"
            | "безопасный"
            | "безопасная"
            | "безопасное"
            | "безопасные"
            | "безопасен"
            | "безопасна"
            | "небезопасно"
            | "небезопасный"
            | "опасен"
            | "доверять"
            | "верить"
            | "развели"
            | "лохотрон"
    ) || RUSSIAN_VERDICT_STEMS
        .iter()
        .any(|stem| word.starts_with(*stem))
}

/// A verb that asks for an action on the screen (any form: "нажимай", "открывай", "clicking").
fn action_word(word: &str) -> bool {
    // English words whole ("Downloads" is no "do"), Russian ones by stem.
    matches!(
        word,
        "click" | "clicking" | "press" | "pressing" | "open" | "opening" | "do" | "tap"
    ) || ["нажм", "нажим", "откр", "сдела", "кликн", "щёлкн", "щелкн"]
        .iter()
        .any(|stem| word.starts_with(stem))
}

/// A command addressed to Mate ("click", "нажми"), not an infinitive ("открыть").
/// Words that only make sense about what is on the screen.
fn screen_word(word: &str) -> bool {
    matches!(
        word,
        "экран"
            | "экране"
            | "экрана"
            | "окне"
            | "вкладка"
            | "вкладке"
            | "вкладку"
            | "всплывающее"
            | "всплыло"
            | "ссылка"
            | "ссылку"
            | "кнопка"
            | "кнопку"
            | "кнопки"
            | "кнопке"
            | "нажать"
            | "нажимать"
            | "кликнуть"
            | "screen"
            | "popup"
            | "button"
            | "click"
    )
}

/// A thing on the computer: "помоги с почтой", "help me print this document".
fn computer_thing(word: &str) -> bool {
    matches!(
        word,
        "email"
            | "emails"
            | "mail"
            | "file"
            | "files"
            | "document"
            | "folder"
            | "printer"
            | "computer"
            | "laptop"
            | "screen"
            | "app"
            | "program"
            | "browser"
            | "website"
            | "internet"
            | "wifi"
            | "settings"
            | "telegram"
            | "whatsapp"
            | "zoom"
            | "почта"
            | "почту"
            | "почтой"
            | "почте"
            | "письмо"
            | "письмом"
            | "файл"
            | "файлы"
            | "файлом"
            | "документ"
            | "папку"
            | "папка"
            | "принтер"
            | "принтером"
            | "компьютер"
            | "компьютере"
            | "компьютером"
            | "ноутбук"
            | "ноутбуке"
            | "экран"
            | "экране"
            | "приложение"
            | "программу"
            | "программа"
            | "браузер"
            | "сайт"
            | "интернет"
            | "интернетом"
            | "вайфай"
            | "настройки"
            | "телеграм"
            | "телеграмом"
            | "ватсап"
    )
}

fn command_word(word: &str) -> bool {
    matches!(
        word,
        "click"
            | "press"
            | "open"
            | "нажми"
            | "нажмите"
            | "открой"
            | "откройте"
            | "сделай"
            | "сделайте"
            | "кликни"
            | "кликните"
            | "щёлкни"
            | "щёлкните"
            | "щелкни"
            | "щелкните"
            | "send"
            | "print"
            | "save"
            | "отправь"
            | "отправьте"
            | "распечатай"
            | "распечатайте"
            | "сохрани"
            | "сохраните"
            | "увеличь"
            | "увеличьте"
            | "уменьши"
            | "уменьшите"
            | "включи"
            | "включите"
            | "выключи"
            | "выключите"
    )
}

/// Conservative English/Russian fallback when the classifier is unavailable. A scam
/// check wins over delegation (Mate looks before anyone presses), delegation over
/// finding, except when the person explicitly asks to learn or how to find it.
pub fn guess_intent(question: &str) -> Intent {
    if asks_safety(question) {
        return Intent::Safety;
    }
    let lower = spoken_words(question);
    let words: Vec<&str> = words(&lower).collect();
    // "Don't click", "не нажимай": a negated action alone is not a request for guidance.
    // Only a negation right before an action word counts: "I can't find the font, it's not
    // there" still finds it.
    // "Don't double-click it" too: "double", "right" and "left" may sit in between.
    let negated_action = (0..words.len()).any(|index| {
        matches!(words[index], "dont" | "not" | "never" | "не" | "никогда")
            && match words.get(index + 1) {
                Some(&("double" | "right" | "left")) => {
                    words.get(index + 2).is_some_and(|word| action_word(word))
                }
                Some(word) => action_word(word),
                None => false,
            }
    });
    let imperative = words
        .iter()
        .find(|word| !matches!(**word, "please" | "mate" | "пожалуйста"))
        .is_some_and(|word| {
            command_word(word)
                || (*word == "make" && words.contains(&"letters") && words.contains(&"bigger"))
                || (*word == "turn" && has_phrase(&words, &["turn", "up", "the", "sound"]))
        });
    // "Не знаю, нажми за меня": a command to Mate anywhere with "за меня", "сам" or
    // "yourself" hands it over (unless an action is negated); "как мне
    // открыть настройки? Я сам хочу разобраться" has no command, so it stays How.
    let handed_over = words.iter().any(|word| command_word(word))
        && (has_phrase(&words, &["за", "меня"])
            || words
                .iter()
                .any(|word| matches!(*word, "сам" | "сами" | "yourself")));
    // Asking about Mate's abilities is conversation, not delegation. Other task
    // cues in the same request still win over this small-talk phrase.
    let for_me = words.windows(2).enumerate().any(|(index, phrase)| {
        phrase == ["for", "me"]
            && !(index >= 4 && words[index - 4..index] == ["what", "can", "you", "do"])
    });
    // A bare "сделай" (or "сделай это", "сделай сам") hands the task over; "сделай шрифт
    // крупнее" is an imperative, so How.
    let do_this = words.iter().enumerate().any(|(index, word)| {
        matches!(*word, "сделай" | "сделайте")
            && words.get(index + 1).is_none_or(|next| {
                matches!(*next, "это" | "сам" | "сами" | "за" | "пожалуйста" | "уже")
            })
    });
    let where_cue = words
        .iter()
        .any(|word| matches!(*word, "where" | "find" | "где" | "найди" | "покажи"))
        || has_phrase(&words, &["show", "me"]);
    // "Помоги мне отправить письмо", "help me send this email": a task on the computer to
    // walk through, so it names a computer thing. "Help me save money", "помоги мне найти
    // силы", "help me write a card" stay conversation.
    let help = (words
        .iter()
        .any(|word| matches!(*word, "помоги" | "помогите"))
        || has_phrase(&words, &["help", "me"]))
        && words.iter().any(|word| computer_thing(word));
    let how_cue = help
        || has_phrase(&words, &["how", "do", "i"])
        || has_phrase(&words, &["how", "to"])
        || has_phrase(&words, &["what", "can", "i", "do", "here"])
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
        });
    let learning = contains_spoken(
        &words,
        &["do it myself", "i want to learn", "so i can learn"],
    );
    if learning {
        return Intent::How;
    }
    if negated_action && !where_cue && !how_cue {
        return Intent::Chat;
    }
    if !negated_action
        && (asks_do_it(question)
            || do_this
            || handed_over
            || has_phrase(&words, &["do", "it"])
            || for_me)
    {
        return Intent::Do;
    }
    if (has_phrase(&words, &["how", "do", "i"]) && words.contains(&"find"))
        || (has_phrase(&words, &["покажи", "как"]) && how_cue)
    {
        return Intent::How;
    }
    if where_cue {
        return Intent::Where;
    }
    if imperative || how_cue {
        return Intent::How;
    }
    // A question about what is in front of the person needs the screen even without
    // "where": "что у меня на экране?", "что мне нажать?", "what does this button do?", a
    // bare "что это?". Without the screen the model can only say it does not see it. Plain
    // "здесь", "тут" or a longer "что это ..." do not count: "у меня тут болит спина, что
    // это может быть?" is no reason for a screenshot (invariant 3).
    let bare_this = matches!(
        words.as_slice(),
        ["что", "это"] | ["а", "что", "это"] | ["what", "is", "this"] | ["whats", "this"]
    );
    // "Расскажи анекдот про кнопку" is still a joke.
    let story = words.iter().any(|word| {
        matches!(
            *word,
            "joke"
                | "jokes"
                | "story"
                | "stories"
                | "poem"
                | "poems"
                | "анекдот"
                | "анекдоты"
                | "анекдота"
                | "история"
                | "историю"
                | "истории"
                | "сказка"
                | "сказку"
                | "сказки"
                | "стих"
                | "стихи"
                | "стишок"
        )
    });
    if bare_this
        || (!story && words.iter().any(|word| screen_word(word)))
        || has_phrase(&words, &["что", "это", "за", "окно"])
        || has_phrase(&words, &["this", "window"])
        // "What should I press here?", "what is this link?", "what's on my monitor?": the
        // plain words are too common alone, these phrasings are about the screen.
        || has_phrase(&words, &["press", "here"])
        || has_phrase(&words, &["i", "press"])
        || (words.len() <= 4 && words.starts_with(&["what", "is", "this"]))
        || (words.len() <= 3 && words.starts_with(&["whats", "this"]))
        || ["monitor", "display"].iter().any(|thing| {
            has_phrase(&words, &["on", "my", thing]) || has_phrase(&words, &["on", "the", thing])
        })
        || has_phrase(&words, &["pop", "up"])
        || has_phrase(&words, &["look", "at", "this"])
        || has_phrase(&words, &["посмотри", "на", "это"])
    {
        return Intent::Where;
    }
    Intent::Chat
}

#[cfg(test)]
#[path = "intent_tests.rs"]
mod tests;
