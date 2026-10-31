use super::{Intent, guess_intent};

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
