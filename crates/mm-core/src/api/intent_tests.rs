use super::{Intent, guess_intent};

#[test]
fn abilities_and_general_help_need_no_screenshot() {
    for question in [
        "what can you do",
        "what are you able to do",
        "how do I use you",
        "who are you?",
        "что ты умеешь",
        "что ты можешь",
        "как тобой пользоваться",
        "помоги разобраться с тобой",
        "help me",
        "помоги",
    ] {
        assert_eq!(guess_intent(question), Intent::Chat, "{question}");
    }
    assert_eq!(guess_intent("How do I print this document?"), Intent::How);
}

#[test]
fn gemini_routing_review_sentences() {
    let mut failures = Vec::new();
    for (text, expected) in [
        ("Я хочу сделать это сам, просто подскажи.", Intent::How),
        ("Не делай за меня, я хочу научиться сам", Intent::How),
        ("Нажми? Нет — покажи, я сам хочу научиться", Intent::How),
        ("Сделай за меня, чтобы я научился", Intent::How),
        // Keep Where: this explicitly asks where the button is, not for a lesson.
        ("Не делай это за меня, покажи где кнопка.", Intent::Where),
        ("Не кликай за меня, я хочу сам научиться", Intent::How),
        ("Не делай это за меня", Intent::Chat),
        ("Не сделай это за меня", Intent::Chat),
        ("Пожалуйста, не нажми за меня", Intent::Chat),
        (
            "Не открывай за меня, расскажи как открыть настройки",
            Intent::How,
        ),
        ("Не надо нажимать за меня", Intent::Chat),
        (
            "Не надо нажимать, сделай за меня пошагово? Нет, я хочу сделать сам — скажи как нажать",
            Intent::How,
        ),
        (
            "Не надо, не трогай, я сам хочу нажать — или нажми?",
            Intent::Chat,
        ),
        (
            "Мне позвонили и сказали, что мою карту заблокировали.",
            Intent::Safety,
        ),
        (
            "Пишут, что моя карточка заблокирована. Это правда?",
            Intent::Safety,
        ),
        (
            "На экране вылезло окно, что у меня вирус на компьютере.",
            Intent::Safety,
        ),
        ("This pop-up says my computer has a virus.", Intent::Safety),
        (
            "Звонят якобы из Сбербанка и просят назвать код из СМС.",
            Intent::Safety,
        ),
        ("позвонили с банка и просят код", Intent::Safety),
        ("просят пароль из смс", Intent::Safety),
        (
            "Тут в письме просят ввести данные карты и три цифры с оборота.",
            Intent::Safety,
        ),
        (
            "Мне пришло сообщение, что я выиграла миллион. Это правда?",
            Intent::Safety,
        ),
        (
            "My daughter bought me this new laptop — isn't that wonderful, or is it too dangerous for my eyes?",
            Intent::Chat,
        ),
        (
            "В интернете пишут, что на улице гололёд и это опасно. Это правда?",
            Intent::Chat,
        ),
        ("интернет говорит, что гололёд — это опасно", Intent::Chat),
        (
            "Я прочитала на сайте, что это лекарство опасное",
            Intent::Chat,
        ),
        (
            "Говорят, что это лекарство опасное, мне страшно",
            Intent::Chat,
        ),
        ("На улице гололёд, мне опасно идти?", Intent::Chat),
        ("Эта погода опасная?", Intent::Chat),
        ("Is that ice on the stairs safe to walk on?", Intent::Chat),
        ("Is that ice on the stairs safe?", Intent::Chat),
        ("This coffee is really good for me, isn't it?", Intent::Chat),
        ("Is this medicine too strong for me?", Intent::Chat),
        ("It is hard for me to see the small letters", Intent::Chat),
        ("It is hard for me to read this", Intent::Chat),
        ("Сделай буквы на экране побольше.", Intent::How),
        ("Сделай, пожалуйста, шрифт крупнее", Intent::How),
        ("Покажи мне, как увеличить шрифт.", Intent::How),
        ("Покажите, как мне открыть почту", Intent::How),
        ("Покажи мне, как открыть почту", Intent::How),
        ("How can I find the Settings page?", Intent::How),
        ("How can I open my email?", Intent::How),
        ("How can I send an email to my grandson?", Intent::How),
    ] {
        let actual = guess_intent(text);
        if actual != expected {
            failures.push(format!("{text}: {actual:?} -> {expected:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn scam_inflections_and_context_guards() {
    for card in ["карта", "карту", "карты", "карточка", "карточку"] {
        assert_eq!(
            guess_intent(&format!("Пишут, что {card} заблокирована")),
            Intent::Safety,
        );
    }
    for message in ["сообщение", "смс", "эсэмэска"] {
        assert_eq!(
            guess_intent(&format!(
                "Пришло {message}, что я выиграла миллион. Это правда?"
            )),
            Intent::Safety,
        );
    }
    for text in [
        "Позвонили из моего банка и просят пароль из смс",
        "В письме просят ввести три цифры с оборота карты",
        "This email asks for my card details",
        "This email asks for the three digits on my card",
        "На экране пишут, что компьютер заражён",
        "Сделай за меня, я хочу научиться сам",
        "Сделай за меня, хочу сам",
        "Покажите, как увеличить шрифт",
    ] {
        let expected = if text.starts_with("Сделай") || text.starts_with("Покажите") {
            Intent::How
        } else {
            Intent::Safety
        };
        assert_eq!(guess_intent(text), expected, "{text}");
    }
    for text in [
        "Моя карточка лежит в кошельке",
        "Мне пришло сообщение от внука",
        "Tell me about viruses",
        "Врач говорит, что у меня вирус",
        "На уроке просят назвать три цифры",
        "Is this weather dangerous?",
        "Эта прогулка безопасна?",
        "Этот ноутбук опасен для глаз?",
    ] {
        assert_eq!(guess_intent(text), Intent::Chat, "{text}");
    }
}

#[test]
fn wonder_review_sentences() {
    // All 30 were characterized before the fix; only groups A-E change routes.
    for (text, expected) in [
        ("Where does my grandson live now?", Intent::Where),
        ("Where can I buy cheap train tickets?", Intent::Where),
        ("Find me a recipe for cabbage soup.", Intent::Where),
        ("Show me the weather for tomorrow.", Intent::Where),
        ("I can’t find my glasses anywhere.", Intent::Where),
        ("Is it safe to walk to the shop on the ice?", Intent::Chat),
        ("На улице гололёд, это безопасно?", Intent::Chat),
        ("Tell me a story about this legit old game.", Intent::Safety),
        (
            "This message says I won money — should I believe it?",
            Intent::Safety,
        ),
        (
            "Мне пришло письмо, что карта заблокирована. Это правда?",
            Intent::Safety,
        ),
        ("Это письмо правда от банка?", Intent::Safety),
        ("Can you open Settings for me?", Intent::Do),
        ("Order these medicines for me, please.", Intent::Do),
        ("Delete this old photo for me.", Intent::Do),
        ("Pay this electricity bill for me.", Intent::Do),
        ("I want to do it myself, just tell me.", Intent::How),
        ("Show me how you do it so I can learn.", Intent::How),
        ("Don’t do it — I want to learn myself.", Intent::How),
        ("Don’t click it for me; show me where it is.", Intent::Where),
        ("How do I find the Settings page?", Intent::How),
        ("Покажи, как мне открыть почту.", Intent::How),
        (
            "Where do I click to attach a file to an email?",
            Intent::Where,
        ),
        ("Make the letters on this page bigger.", Intent::How),
        ("Turn up the sound, I can’t hear the video.", Intent::How),
        ("Close this annoying pop-up for me.", Intent::Do),
        (
            "This window keeps popping up saying my computer is infected.",
            Intent::Safety,
        ),
        (
            "Someone rang saying they are from the bank and need a code from my phone.",
            Intent::Safety,
        ),
        ("The printer will not print my letter.", Intent::Chat),
        ("Settings will not open when I click them.", Intent::Chat),
        ("Помоги мне разобраться, ничего не понимаю.", Intent::Chat),
    ] {
        assert_eq!(guess_intent(text), expected, "{text}");
    }
}

#[test]
fn scam_reports_need_context_and_learning_never_delegates() {
    for (text, expected) in [
        ("карта заблокирована. Это правда?", Intent::Safety),
        ("Мне звонят из банка и просят код", Intent::Safety),
        (
            "The call was from the bank and it asked for a code",
            Intent::Safety,
        ),
        (
            "An email says it is blocked. Should I believe it?",
            Intent::Safety,
        ),
        ("My computer is infected", Intent::Safety),
        ("Is it safe to open this email?", Intent::Safety),
        ("Это безопасно открыть на компьютере?", Intent::Safety),
        ("Should I believe it?", Intent::Chat),
        ("Is it safe on ice?", Intent::Chat),
        ("На льду это безопасно?", Intent::Chat),
        ("Is it safe to do yoga on ice?", Intent::Chat),
        ("My cut is infected", Intent::Chat),
        ("The road is blocked", Intent::Chat),
        ("The bank is open", Intent::Chat),
        ("Tell me about code", Intent::Chat),
        ("Don't do it, I want to learn myself", Intent::How),
        ("Do it for me so I can learn", Intent::How),
        (
            "Is this safe? Show me how you do it so I can learn",
            Intent::Safety,
        ),
        ("Покажи, как открыть почту", Intent::How),
        ("How do I find Save?", Intent::How),
        ("увеличь шрифт", Intent::How),
        ("Please make the letters on this page bigger", Intent::How),
        ("Mate, turn up the sound", Intent::How),
    ] {
        assert_eq!(guess_intent(text), expected, "{text}");
    }
}

#[test]
fn kimi_routing_v5_review_sentences() {
    let mut failures = Vec::new();
    for (text, expected) in [
        (
            "Мне написали, что надо перевести деньги на безопасный счёт",
            Intent::Safety,
        ),
        (
            "Звонят и говорят перевести всё на защищённый счет",
            Intent::Safety,
        ),
        ("В смс просят перейти по ссылке", Intent::Safety),
        (
            "This text asks me to click a link to get my parcel",
            Intent::Safety,
        ),
        ("Пришла смс о блокировке карты", Intent::Safety),
        ("Я не хочу сам, сделай за меня", Intent::Do),
        ("Is this laptop dangerous for my vision?", Intent::Chat),
        ("Этот планшет опасен для зрения?", Intent::Chat),
        // Unchanged neighbours: a plain transfer is a lesson, a learning wish stays How.
        ("Как перевести деньги внуку?", Intent::How),
        ("Не делай за меня, я хочу научиться сам", Intent::How),
    ] {
        let got = guess_intent(text);
        if got != expected {
            failures.push(format!("{text:?}: expected {expected:?}, got {got:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
}

#[test]
fn meta_routing_v5_review_sentences() {
    let mut failures = Vec::new();
    for (text, expected) in [
        (
            "My grandson asked me to open a link to his holiday photos.",
            Intent::Chat,
        ),
        ("Is it safe outside today, or is it icy?", Intent::Chat),
        (
            "A caller asked for the three numbers on the back of my card.",
            Intent::Safety,
        ),
        (
            "They say my card will be blocked tonight unless I ring this number.",
            Intent::Safety,
        ),
        (
            "A man phoned and says I must move my money to a protected account.",
            Intent::Safety,
        ),
        ("Не надо объяснять, просто сделай это.", Intent::Do),
        (
            "No, don't talk me through it, just close it for me.",
            Intent::Do,
        ),
        // Still refused or a lesson, as before.
        ("Не надо нажимать за меня", Intent::Chat),
        ("Не делай за меня, я хочу научиться сам", Intent::How),
        ("В смс просят перейти по ссылке", Intent::Safety),
    ] {
        let got = guess_intent(text);
        if got != expected {
            failures.push(format!("{text:?}: expected {expected:?}, got {got:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
}

#[test]
fn more_ways_to_ask_about_mate_take_no_screenshot() {
    for question in [
        "Mate, what else can you do?",
        "How can you help me?",
        "Can you tell me what you can do?",
        "Help",
        "Привет, что ещё ты умеешь?",
        "Чем ты можешь мне помочь?",
        "Расскажи о себе",
    ] {
        assert_eq!(
            super::guess_intent(question),
            super::Intent::Chat,
            "{question}"
        );
    }
}
