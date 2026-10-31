//! Label-based password detection from `tools/uia_dump.ps1` (D37, invariant 3), and the
//! "do it for me" blocklist (`forbidden`, invariant 4).
//! Revealing a password clears IsPassword, so callers must also check these signals.

use std::sync::LazyLock;

use regex::Regex;

#[cfg(test)]
mod wp87_tests {
    #[test]
    fn revealed_keys_and_multilingual_password_labels_are_sensitive() {
        for word in [
            "security key",
            "network key",
            "network security key",
            "ключ безопасности",
            "ключ сети",
            "show characters",
            "отображать вводимые знаки",
            "clave",
            "код подтверждения",
            "құпия сөз",
            "сырсөз",
            "şifrə",
            "PIN",
            "OTP",
        ] {
            assert!(super::password_like("Edit", word, "", "", ""), "{word}");
            assert!(super::password_like("Custom", word, "", "", ""), "{word}");
        }
        assert!(!super::password_like("Custom", "spinning", "", "", ""));
    }
}

static PASSWORD_LONG: LazyLock<Regex> = LazyLock::new(|| {
    // This constant pattern is validated by the privacy tests; compilation cannot fail.
    Regex::new(concat!(
        r"(?i)password|passwd|passcode|passphrase|\bparol|",
        r"one[ -]time[ -]code|verification\s+code|security\s+code|",
        r"security\s+key|network\s+key|network\s+security\s+key|show\s+characters|",
        r"ключ\s+безопасности|ключ\s+сети|отображать\s+вводимые\s+знаки|",
        r"clave|код\s+подтверждения|құпия\s+сөз|сырсөз|şifrə|",
        r"пароль|пин-код|kennwort|contraseña|mot\s+de\s+passe|senha|",
        r"[şs]ifre|hasło|heslo|wachtwoord|lösenord|salasana|jelszó|parola|",
        r"κωδικός|密码|密碼|パスワード|비밀번호|",
        r"كلمة\s+المرور|סיסמה|mật\s+khẩu|kata\s+sandi|รหัสผ่าน",
    ))
    .expect("constant password regex must compile")
});

/// Detect multilingual password words and short tokens in an edit field's labels.
pub fn password_words(text: &str) -> bool {
    password_toggle(text) || short_token(text)
}

/// Detect long password words that can identify a reveal control.
pub fn password_toggle(text: &str) -> bool {
    PASSWORD_LONG.is_match(text)
}

fn short_token(text: &str) -> bool {
    for (start, _) in text.char_indices() {
        let before = text[..start].chars().next_back();
        for token in ["pwd", "pin", "otp", "cvv", "cvc", "пин"] {
            let end = start + token.len();
            let Some(candidate) = text.get(start..end) else {
                continue;
            };
            let cyrillic = token == "пин";
            let matches = if cyrillic {
                candidate
                    .chars()
                    .flat_map(char::to_lowercase)
                    .eq(token.chars())
            } else {
                candidate.eq_ignore_ascii_case(token)
            };
            if !matches {
                continue;
            }
            let after = text[end..].chars().next();
            let blocked = |neighbour: char| {
                if cyrillic {
                    // Unlike the ASCII lookarounds, these inherit PowerShell's (?i).
                    matches!(neighbour, 'а'..='я' | 'ё' | 'А'..='Я' | 'Ё')
                } else {
                    neighbour.is_ascii_lowercase()
                }
            };
            if !before.is_some_and(blocked) && !after.is_some_and(blocked) {
                return true;
            }
        }
    }
    false
}

/// Classify UIA metadata without reading the control's value or IsPassword flag.
pub fn password_like(
    control_type: &str,
    name: &str,
    automation_id: &str,
    help_text: &str,
    class_name: &str,
) -> bool {
    if class_name.eq_ignore_ascii_case("PasswordBox") {
        return true;
    }
    if control_type.eq_ignore_ascii_case("Edit") || control_type.eq_ignore_ascii_case("Custom") {
        return (control_type.eq_ignore_ascii_case("Custom")
            && automation_id.eq_ignore_ascii_case("RevealButton"))
            || [name, automation_id, help_text]
                .into_iter()
                .any(password_words);
    }
    if ["Text", "Document", "Window", "Pane"]
        .into_iter()
        .any(|kind| control_type.eq_ignore_ascii_case(kind))
    {
        return false;
    }
    automation_id.eq_ignore_ascii_case("RevealButton")
        || password_toggle(name)
        || password_toggle(automation_id)
}

/// Lowercase stems of actions Mate never performs for the user (invariant 4): paying,
/// buying, ordering, deleting, moving money. ASCII stems match at the start of a word
/// ("Pay now", "btnDelete", never "Voice Recorder"); Cyrillic stems match anywhere,
/// because Russian puts prefixes in front of them ("предоплата", "покупка").
const FORBIDDEN_STEMS: [&str; 41] = [
    "pay",
    "prepay",
    "repay",
    "buy",
    "purchase",
    "purchasing",
    "order",
    "reorder",
    "checkout",
    "check out",
    "delete",
    "remove",
    "uninstall",
    "transfer",
    "send money",
    "donate",
    "donation",
    "withdraw",
    "deposit",
    "top up",
    "topup",
    "remit",
    "erase",
    "wipe",
    "trash",
    "reset",
    "clear data",
    "clear history",
    "оплат",
    "заплат",
    "платёж",
    "платеж",
    "купи",
    "куплю",
    "покуп",
    "заказ",
    "удал",
    "перевод",
    "перевести деньги",
    "отправить деньги",
    "пожертв",
];

/// Cyrillic stems that must start a word: inside a word they mean something harmless
/// ("расписание" holds "списа", "подсброс" is no reset).
const FORBIDDEN_WORD_STARTS: [&str; 9] = [
    "оформит",
    "списа",
    "перевест",
    "вывест",
    "пополн",
    "стереть",
    "уничтож",
    "очист",
    "сброс",
];

const FORBIDDEN_EXTRA: &[&str] = &[
    "pag",
    "sil",
    "compr",
    "o'tkaz",
    "discard",
    "subscribe",
    "subscription",
    "preorder",
    "gpay",
    "google pay",
    "apple pay",
    "lösch",
    "bezahl",
    "kauf",
    "überweis",
    "supprim",
    "payer",
    "achet",
    "virement",
    "elimin",
    "borr",
    "transferir",
    "o'chir",
    "to'la",
    "sotib ol",
    "öde",
    "satın al",
    "havale",
    "жою",
    "жой",
    "төле",
    "сатып ал",
    "аудар",
    "подписаться",
    "видалити",
    "оплатити",
    "купити",
    "переказати",
];

// Turkish uppercase dotted I is not an ASCII case fold of the broad "sil" stem.
static TURKISH_DELETE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bs[iİ]l").expect("constant Turkish delete regex must compile")
});

static MONEY_AMOUNT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)(?:\p{Sc}|so'm\b|сум\b|руб\b|тенге\b)\s*\d|",
        r"\d[\d\s.,]*\s*(?:\p{Sc}|so'm\b|сум\b|руб\b|тенге\b)|",
        r"\bотправить\s+\d",
    ))
    .expect("constant money amount regex must compile")
});

static FORBIDDEN_PHRASES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\bclear\b.*\b(?:data|history|now|browsing)\b|",
        r"\bempty\s+(?:recycle\s+)?bin\b|",
        r"\bmove\s+to\s+(?:the\s+)?(?:bin|trash)\b|",
        r"\bsend\s+(?:\p{Sc}|[0-9]|(?:usd|eur|gbp|rub|uzs|kzt|try|dollars?|euros?|pounds?|roubles?|tenge|som)\b)",
    )).expect("constant forbidden phrase regex must compile")
});

/// Whether clicking a control labelled `name` could pay, buy, order, delete, move money or
/// touch a password (invariant 4). `control_type` is the UIA control type name: short
/// tokens such as "PIN" only count in an edit field, as in `password_like`. Callers check
/// every label they have (name, automation id); a match means never click it for the user.
pub fn forbidden(name: &str, control_type: &str) -> bool {
    let field = ["Edit", "Custom", "Document"]
        .iter()
        .any(|kind| control_type.eq_ignore_ascii_case(kind));
    if password_toggle(name) || (field && password_words(name)) {
        return true;
    }
    let text = separated(name);
    FORBIDDEN_PHRASES.is_match(&text)
        || TURKISH_DELETE
            .find_iter(&text)
            .any(|matched| !harmless_stem_word(&text[matched.start()..], "sil"))
        || MONEY_AMOUNT.is_match(&text)
        || FORBIDDEN_EXTRA
            .iter()
            .any(|stem| contains_stem(&text, stem))
        || FORBIDDEN_STEMS
            .into_iter()
            .any(|stem| contains_stem(&text, stem))
        || FORBIDDEN_WORD_STARTS
            .into_iter()
            .any(|stem| contains_word_start(&text, stem))
}

/// `name` with every run of whitespace, hyphens and underscores as one space ("Check-out",
/// "send_money"), without leading or trailing gaps.
fn separated(name: &str) -> String {
    let mut text = String::with_capacity(name.len());
    let mut gap = false;
    for c in name.chars() {
        if c.is_whitespace() || c == '-' || c == '_' {
            gap = true;
            continue;
        }
        if gap && !text.is_empty() {
            text.push(' ');
        }
        gap = false;
        text.push(if matches!(c, '’' | '‘' | 'ʻ' | 'ʼ') {
            '\''
        } else {
            c
        });
    }
    text
}

/// Case-insensitive; an ASCII stem must also start a word.
fn contains_stem(text: &str, stem: &str) -> bool {
    text.char_indices().any(|(start, first)| {
        let Some(candidate) = text.get(start..start + stem.len()) else {
            return false;
        };
        candidate
            .chars()
            .flat_map(char::to_lowercase)
            .eq(stem.chars())
            && (!stem.is_ascii() || word_start(text[..start].chars().next_back(), first))
            && !harmless_stem_word(&text[start..], stem)
    })
}

/// Exceptions apply to one complete word, never to the rest of the label.
fn harmless_stem_word(text: &str, stem: &str) -> bool {
    let exceptions: &[&str] = match stem {
        "pag" => &["page", "pages", "paging"],
        "sil" => &["silent", "silence", "silently"],
        "compr" => &["compress", "compression"],
        "o'tkaz" => &["o'tkazib"],
        _ => return false,
    };
    exceptions.iter().any(|word| {
        text.get(..word.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(word))
            && text[word.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric())
    })
}

/// Case-insensitive, at the start of a word only, whatever the script.
fn contains_word_start(text: &str, stem: &str) -> bool {
    text.char_indices().any(|(start, first)| {
        text.get(start..start + stem.len())
            .is_some_and(|candidate| {
                candidate
                    .chars()
                    .flat_map(char::to_lowercase)
                    .eq(stem.chars())
            })
            && word_start(text[..start].chars().next_back(), first)
    })
}

/// A word starts after anything but a letter or digit, or at a camelCase hump.
fn word_start(before: Option<char>, first: char) -> bool {
    before.is_none_or(|before| {
        !before.is_alphanumeric() || (before.is_lowercase() && first.is_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_delete_and_password_controls_are_forbidden() {
        for name in [
            "Pay now",
            "PayPal",
            "Prepay",
            "Buy now",
            "Buy it again",
            "Complete purchase",
            "Place order",
            "Reorder",
            "Checkout",
            "Proceed to check-out",
            "Delete",
            "btnDelete",
            "DELETE_ACCOUNT",
            "Remove from cart",
            "Uninstall",
            "Transfer funds",
            "Send  money",
            "send_money",
            "Donate",
            "Withdraw",
            "Оплатить",
            "Предоплата",
            "ПЛАТЁЖ",
            "Платеж",
            "Подтвердить платёж",
            "Купить",
            "Покупки",
            "Оформить заказ",
            "Удалить",
            "Перевод",
            "Перевести",
            "Перевести  деньги",
            "Отправить деньги",
            "Пожертвовать",
            "Show password",
            "Показать пароль",
            "Пароль",
            "Top up",
            "Deposit funds",
            "Empty Trash",
            "Erase all content",
            "Wipe device",
            "Reset this PC",
            "Clear history",
            "Оформить",
            "Списать",
            "Перевести 500 ₽",
            "Вывести",
            "Пополнить",
            "Оформить подписку",
            "Перевести по СБП",
            "Стереть всё",
            "Уничтожить",
            "Очистить историю",
            "Сбросить",
        ] {
            assert!(forbidden(name, "Button"), "expected forbidden: {name}");
        }
    }

    #[test]
    fn everyday_controls_are_not_forbidden() {
        for name in [
            "",
            "Display",
            "Border",
            "Voice Recorder",
            "Papaya theme",
            "Removable disk",
            "Replay",
            "Zoom in",
            "Print",
            "New tab",
            "Volume",
            "Text size",
            "Wi-Fi",
            "Pin tab",
            "Стереорежим",
            "Платформа",
            "Показать",
            "Настройки",
            "Громкость",
            "Печать",
            "Wireless display",
            "Оформление",
            "Список",
            "Дополнительно",
            "Отправить по расписанию",
            "Подпись",
        ] {
            assert!(!forbidden(name, "Button"), "unexpected forbidden: {name}");
        }
    }

    #[test]
    fn short_password_tokens_count_only_in_edit_fields() {
        for name in ["PIN", "otp_input", "CVV", "пин"] {
            assert!(forbidden(name, "Edit"), "expected forbidden: {name}");
            assert!(forbidden(name, "Custom"), "expected forbidden: {name}");
            assert!(forbidden(name, "edit"), "expected forbidden: {name}");
            assert!(!forbidden(name, "Button"), "unexpected forbidden: {name}");
        }
        assert!(forbidden("Password", "Text"));
    }

    #[test]
    fn destructive_phrases_subscriptions_and_multilingual_actions_are_forbidden() {
        for name in [
            "Clear browsing data",
            "Clear now",
            "Clear all history",
            "Clear saved data",
            "Empty Recycle Bin",
            "Empty bin",
            "Discard",
            "Move to Bin",
            "Move to the trash",
            "Send $20",
            "Send 20",
            "Send €5",
            "Send USD 20",
            "Subscribe",
            "Subscription",
            "Preorder",
            "GPay",
            "Google Pay",
            "Apple Pay",
            "Löschen",
            "Bezahlen",
            "Kaufen",
            "Überweisen",
            "Supprimer",
            "Payer",
            "Acheter",
            "Virement",
            "Eliminar",
            "Borrar",
            "Pagar",
            "Comprar",
            "Transferir",
            "O'chirish",
            "To'lash",
            "Sotib olish",
            "O'tkazma",
            "O‘chirish",
            "Toʻlash",
            "Sil",
            "Öde",
            "Satın al",
            "Havale",
            "Жою",
            "Төлеу",
            "Сатып алу",
            "Аудару",
        ] {
            assert!(forbidden(name, "Button"), "expected forbidden: {name}");
        }
        for name in [
            "Clear selection",
            "Send feedback",
            "Open calendar",
            "Find next",
            "Help",
        ] {
            assert!(!forbidden(name, "Button"), "unexpected forbidden: {name}");
        }
    }

    #[test]
    fn ascii_stems_start_a_word_and_cyrillic_stems_match_anywhere() {
        assert!(forbidden("Proceed (pay)", "Button"));
        assert!(forbidden("Step 2: Delete", "Hyperlink"));
        assert!(!forbidden("Taxpayer info", "Button"));
        assert!(!forbidden("undelete", "Button"));
        assert!(forbidden("Предоплатить", "Button"));
        assert!(forbidden("Нажмите, чтобы удалить", "Text"));
    }
}
