//! "What is going on on this page?", "Что на этой странице?": the person asks Mate to look
//! at the window in front and explain it, not to find one thing in it (D80).

use super::contains_spoken;

pub(super) fn asks_describe(words: &[&str]) -> bool {
    english(words) || russian(words)
}

/// The window in front, by name.
fn view(word: &str) -> bool {
    matches!(
        word,
        "page" | "webpage" | "site" | "website" | "window" | "tab" | "screen"
    )
}

/// Something with words to read out: "what does this message say?".
fn text(word: &str) -> bool {
    view(word) || matches!(word, "message" | "email" | "letter" | "text" | "article")
}

/// "This page", "the screen", "my tab".
fn names_view(words: &[&str]) -> bool {
    words
        .windows(2)
        .any(|pair| matches!(pair[0], "this" | "that" | "the" | "my") && view(pair[1]))
}

/// "What can you see on this page?": the same words with the window named.
fn saw_named(words: &[&str]) -> bool {
    contains_spoken(
        words,
        &[
            "what can you see",
            "what you can see",
            "what do you see",
            "what you see",
        ],
    )
}

fn english(words: &[&str]) -> bool {
    // "What is going on with my back?" names no window: a chat.
    let happening = contains_spoken(
        words,
        &[
            "what is going on",
            "whats going on",
            "what is happening",
            "whats happening",
        ],
    );
    // "What can you see?" is said to Mate, not about the world: "what do you see in a dream?"
    // and "what can you see from the moon?" stay a chat, with no look.
    let seeing = contains_spoken(
        words,
        &[
            "what can you see",
            "what you can see",
            "what do you see",
            "what you see",
        ],
    ) && matches!(
        words.last(),
        Some(&("see" | "here" | "now" | "please" | "mate"))
    );
    (happening && names_view(words))
        || seeing
        || (saw_named(words) && names_view(words))
        || contains_spoken(
            words,
            &[
                "what am i looking at",
                "what are we looking at",
                "describe what you see",
                "tell me what you see",
                "what is written here",
                "whats written here",
            ],
        )
        || (0..words.len()).any(|at| match &words[at..] {
            // "What's on this page?"; "what's on my screen?" stays the plain look it was.
            ["whats", "on", owner, place, ..] | ["what", "is", "on", owner, place, ..]
                if matches!(*owner, "this" | "that" | "the" | "my")
                    && view(place)
                    && *place != "screen" =>
            {
                true
            }
            // "What is this page?", "what site is this?"; "what is this link?" is no window.
            ["what", "is", "this" | "that", place, ..] | ["whats", "this" | "that", place, ..]
                if view(place) =>
            {
                true
            }
            ["what", place, "is", "this" | "that", ..] if view(place) => true,
            ["what", "does", "it" | "this" | "that", "say", ..] => true,
            ["what", "does", owner, thing, "say", ..]
                if matches!(*owner, "this" | "that" | "the" | "my") && text(thing) =>
            {
                true
            }
            [
                "explain" | "describe" | "summarize" | "summarise" | "summary" | "written",
                rest @ ..,
            ] if names_view(rest) => true,
            ["describe" | "summarize" | "summarise", "this", rest @ ..]
                if rest
                    .first()
                    .is_none_or(|next| matches!(*next, "to" | "for" | "please")) =>
            {
                true
            }
            ["tell", "me", "what" | "about", rest @ ..] if names_view(rest) => true,
            // "Please read this to me"; "it is hard for me to read this" asks for nothing.
            ["read", rest @ ..] => {
                words[..at].iter().all(|word| addressing(word)) && reads_out(rest)
            }
            _ => false,
        })
}

/// Words before a request to Mate: "Mate, can you please read this?".
fn addressing(word: &str) -> bool {
    matches!(
        word,
        "please" | "mate" | "hey" | "hi" | "can" | "could" | "would" | "will" | "you" | "just"
    )
}

/// "Read this (to me)", "read me the message"; "read me a story" and "read this book" are not
/// about the window.
fn reads_out(rest: &[&str]) -> bool {
    let rest = match rest {
        ["me", rest @ ..] => rest,
        _ => rest,
    };
    match rest {
        ["this" | "that"] => true,
        ["this" | "that", next, ..] => {
            text(next) || matches!(*next, "to" | "for" | "aloud" | "out" | "please")
        }
        ["it", "to" | "for" | "aloud" | "out", ..] => true,
        ["the" | "my", thing, ..] => text(thing),
        _ => false,
    }
}

/// The window in front in Russian; "за окном" (outdoors) is no window.
fn russian_view(word: &str) -> bool {
    matches!(
        word,
        "страница"
            | "страницу"
            | "странице"
            | "страничка"
            | "страничку"
            | "страничке"
            | "сайт"
            | "сайте"
            | "экран"
            | "экране"
            | "окно"
            | "окне"
            | "вкладка"
            | "вкладку"
            | "вкладке"
    )
}

fn russian(words: &[&str]) -> bool {
    // "На экране", "в этом окне", "эту страницу"; "какой сайт лучше?" names no window in front.
    let shown = words.windows(2).any(|pair| {
        matches!(
            pair[0],
            "на" | "в" | "во" | "это" | "эта" | "эту" | "этой" | "этом" | "этот" | "этого"
        ) && russian_view(pair[1])
    });
    let here = shown || contains_spoken(words, &["что тут", "что здесь", "что ты видишь"]);
    let reading = contains_spoken(
        words,
        &[
            "прочитай",
            "прочитайте",
            "прочти",
            "прочтите",
            "зачитай",
            "зачитайте",
        ],
    );
    // "Что ты видишь во сне?" stays a chat, with no look.
    let seeing = contains_spoken(
        words,
        &[
            "что ты видишь",
            "что видишь",
            "что ты тут видишь",
            "что ты здесь видишь",
        ],
    ) && (shown
        || matches!(
            words.last(),
            Some(&("видишь" | "тут" | "здесь" | "сейчас" | "пожалуйста"))
        ));
    seeing
        || (contains_spoken(words, &["происходит", "творится"]) && shown)
        || (contains_spoken(words, &["объясни", "объясните", "поясни", "поясните"]) && here)
        || (reading && (here || contains_spoken(words, &["это", "написано"])))
        || (words.contains(&"написано") && here)
        || (contains_spoken(words, &["расскажи", "расскажите"]) && here)
        || (contains_spoken(words, &["опиши", "опишите"])
            && (here
                || words
                    .iter()
                    .any(|word| *word == "это" || russian_view(word))))
        || contains_spoken(words, &["на что я смотрю", "на что мы смотрим"])
        || (0..words.len()).any(|at| {
            let rest = &words[at..];
            // "Что на этой странице?"; "что у меня на экране?" stays the plain look it was.
            matches!(
                rest,
                ["что", "на" | "в" | "во", "этой" | "этом", place, ..]
                    | ["что", "на" | "в" | "во", place, ..]
                    if matches!(*place, "странице" | "страничке" | "сайте" | "окне" | "вкладке")
            ) || matches!(
                rest,
                ["что", "это", "за", place, ..] | ["что", "за", place, ..]
                    if matches!(*place, "страница" | "страничка" | "сайт" | "вкладка")
            )
        })
}
