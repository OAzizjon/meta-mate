//! Questions about Mate alone need no screen. A combined task still uses task routing.

pub(super) fn asks_about_mate(words: &[&str]) -> bool {
    let question = words
        .iter()
        .copied()
        .filter(|word| {
            !matches!(
                *word,
                "please"
                    | "пожалуйста"
                    | "mate"
                    | "мейт"
                    | "hey"
                    | "hi"
                    | "привет"
                    | "so"
                    | "а"
                    | "ну"
            )
        })
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
            | "what else can you do"
            | "what can you help me with"
            | "how can you help me"
            | "how can you help"
            | "what do you do"
            | "tell me what you can do"
            | "can you tell me what you can do"
            | "what are you"
            | "help"
            | "что ещё ты умеешь"
            | "что еще ты умеешь"
            | "чем ты можешь помочь"
            | "чем ты можешь мне помочь"
            | "что ты делаешь"
            | "расскажи о себе"
            | "кто ты такой"
            | "что ты такое"
            | "помощь"
            | "помоги"
    )
}
