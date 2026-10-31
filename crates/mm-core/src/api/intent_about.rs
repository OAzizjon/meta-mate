//! Questions about Mate alone need no screen. A combined task still uses task routing.

pub(super) fn asks_about_mate(words: &[&str]) -> bool {
    let question = words
        .iter()
        .copied()
        .filter(|word| !matches!(*word, "please" | "пожалуйста"))
        .collect::<Vec<_>>()
        .join(" ");
    matches!(
        question.as_str(),
        "what can you do"
            | "what are you able to do"
            | "how do i use you"
            | "who are you"
            | "how to use you"
            | "что ты умеешь"
            | "что ты можешь"
            | "кто ты"
            | "как тобой пользоваться"
            | "помоги разобраться с тобой"
    )
}
