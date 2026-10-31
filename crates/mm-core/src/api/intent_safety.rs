use super::{
    VERDICT_REACH, computer_thing, contains_spoken, is_subject, is_verdict, screen_word,
    spoken_words, words,
};

/// "Is this email real?", "Это мошенники?": a verdict on trust with the thing on the
/// screen (or the person it is aimed at) at most a few words either side of it. A verdict word
/// alone never counts ("Tell me about phone scams"), nor a setting named after one
/// ("safe mode"); "safety" and "безопасности" are no verdicts ("Where is the safety
/// settings?").
pub(super) fn asks_safety(text: &str) -> bool {
    let lower = spoken_words(text);
    let words: Vec<_> = words(&lower).collect();
    let screen_subject =
        words.iter().any(|word| {
            (computer_thing(word) && !matches!(*word, "internet" | "интернет"))
                || screen_word(word)
                || word.starts_with("нажим")
                || matches!(
                    *word,
                    "message"
                        | "window"
                        | "page"
                        | "link"
                        | "card"
                        | "сообщение"
                        | "сообщения"
                        | "смс"
                        | "эсэмэска"
                        | "письме"
                )
                || russian_card(word)
        }) || contains_spoken(&words, &["really from", "действительно от", "правда от"]);
    // A bare safety question keeps its context even beside another request.
    let bare_safety = lower.split(['?', '.', '!']).any(|clause| {
        let clause: Vec<_> = self::words(clause).collect();
        !clause.is_empty()
            && clause.iter().enumerate().all(|(at, word)| {
                is_subject(word)
                    || is_verdict(&clause, at)
                    || matches!(
                        *word,
                        "is" | "are" | "a" | "ли" | "не" | "привет" | "hello" | "mate"
                    )
            })
    });
    let credentials = words
        .iter()
        .any(|w| matches!(*w, "code" | "password" | "код" | "пароль"));
    let requested = words.iter().any(|w| {
        w.starts_with("прос")
            || w.starts_with("попрос")
            || matches!(*w, "ask" | "asks" | "asked" | "asking")
    });
    let infected_computer = words.iter().any(|w| {
        matches!(*w, "infected" | "virus" | "вирус")
            || w.starts_with("заражен")
            || w.starts_with("заражён")
    }) && words
        .iter()
        .any(|w| matches!(*w, "computer" | "компьютер" | "компьютере"));
    // "заблокировали", and the noun too: "смс о блокировке карты".
    // "заблокировали", "смс о блокировке карты", "my card will be blocked".
    let blocked_card = words
        .iter()
        .any(|w| russian_card(w) || matches!(*w, "card" | "cards"))
        && words.iter().any(|w| {
            w.starts_with("заблок")
                || w.starts_with("блок")
                || matches!(*w, "blocked" | "block" | "frozen" | "suspended" | "locked")
        });
    // The "safe account" scam is named by its own words; a link someone asks to open is a
    // message to check, wherever it came from.
    let plain = lower.replace('ё', "е");
    let plain_words: Vec<_> = self::words(&plain).collect();
    let safe_account = contains_spoken(
        &plain_words,
        &[
            "безопасный счет",
            "безопасного счета",
            "защищенный счет",
            "резервный счет",
            "safe account",
            "secure account",
            "protected account",
            "reserve account",
        ],
    );
    // Only a link a message or a caller asks to open: a grandson's photo link is a chat.
    let message = words.iter().any(|w| {
        matches!(
            *w,
            "смс" | "sms" | "text" | "message" | "email" | "e-mail" | "сообщение" | "сообщении"
        ) || w.starts_with("письм")
            || w.starts_with("звон")
            || w.starts_with("call")
    });
    let link_request = requested
        && message
        && contains_spoken(
            &words,
            &[
                "по ссылке",
                "click a link",
                "click the link",
                "open a link",
                "open the link",
            ],
        );
    let credential_request = requested
        && (contains_spoken(
            &words,
            &[
                "данные карты",
                "три цифры",
                "card details",
                "three digits",
                "three numbers",
                "back of my card",
            ],
        ) || (credentials
            && words
                .iter()
                .any(|w| matches!(*w, "смс" | "эсэмэска" | "sms"))));
    let from_bank = contains_spoken(
        &words,
        &["from the bank", "от банка", "из банка", "с банка"],
    ) || (words.iter().any(|w| w.ends_with("банка"))
        && words.iter().any(|w| matches!(*w, "из" | "от" | "с")));
    if (screen_subject
        && (contains_spoken(&words, &["believe it", "это правда"])
            || infected_computer
            || blocked_card
            || credential_request))
        || (credentials && from_bank)
        || safe_account
        || link_request
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
                // Physical danger needs screen context; eye health is conversation.
                && (!physical_danger(words[at])
                    || ((screen_subject || bare_safety)
                        && !words.iter().any(|w| {
                            matches!(*w, "eyes" | "eyesight" | "vision" | "sight")
                                || w.starts_with("глаз")
                                || w.starts_with("зрен")
                        })))
                && !words
                    .get(at + 1)
                    .is_some_and(|next| matches!(*next, "mode" | "quick" | "quickly" | "режим"))
                && words
                    [at.saturating_sub(VERDICT_REACH)..(at + VERDICT_REACH + 1).min(words.len())]
                    .iter()
                    .any(|word| is_subject(word))
        })
}

fn russian_card(word: &str) -> bool {
    matches!(word, "карта" | "карту" | "карты" | "карте" | "картой") || word.starts_with("карточк")
}

fn physical_danger(word: &str) -> bool {
    matches!(
        word,
        "safe" | "unsafe" | "dangerous" | "опасен" | "безопасен" | "небезопасен"
    ) || ["опасн", "безопасн", "небезопасн"]
        .iter()
        .any(|stem| word.starts_with(stem))
}
