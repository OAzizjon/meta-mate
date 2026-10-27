//! Label-based password detection from `tools/uia_dump.ps1` (D37, invariant 3).
//! Revealing a password clears IsPassword, so callers must also check these signals.

use std::sync::LazyLock;

use regex::Regex;

static PASSWORD_LONG: LazyLock<Regex> = LazyLock::new(|| {
    // This constant pattern is validated by the privacy tests; compilation cannot fail.
    Regex::new(concat!(
        r"(?i)password|passwd|passcode|passphrase|\bparol|",
        r"one[ -]time[ -]code|verification\s+code|security\s+code|",
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
    if control_type.eq_ignore_ascii_case("Edit") {
        return [name, automation_id, help_text]
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
