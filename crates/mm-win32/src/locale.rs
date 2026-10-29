//! Windows display language before any WebView subscribes.
use windows::Win32::Globalization::GetUserDefaultUILanguage;

pub fn system_locale() -> &'static str {
    // SAFETY: this query takes no pointers and has no preconditions.
    from_langid(unsafe { GetUserDefaultUILanguage() })
}

fn from_langid(language: u16) -> &'static str {
    // LANGID's low ten bits identify the primary language (Russian = 0x19).
    if language & 0x03ff == 0x19 {
        "ru"
    } else {
        "en"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_display_languages_and_english_fallback() {
        assert_eq!(from_langid(0x0419), "ru");
        assert_eq!(from_langid(0x0819), "ru");
        assert_eq!(from_langid(0x0409), "en");
        assert_eq!(from_langid(0x0407), "en");
    }
}
