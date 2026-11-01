//! Start Menu matching uses only the person's request, never model output or paths.

#[derive(Debug, PartialEq, Eq)]
pub enum Match {
    None,
    One(usize),
    Ambiguous,
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('ё', "е")
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| {
            match word {
                "телеграм" => "telegram",
                "ворд" => "word",
                "эксель" => "excel",
                "зум" => "zoom",
                "скайп" => "skype",
                "вотсап" | "ватсап" => "whatsapp",
                "вайбер" => "viber",
                "хром" => "chrome",
                "энидеск" => "anydesk",
                "тимвьювер" => "teamviewer",
                _ => word,
            }
            .to_owned()
        })
        .collect()
}

/// Whole-app commands only; screen-relative controls and settings pages stay guidance.
pub fn requested_app(question: &str) -> Option<String> {
    if !crate::api::intent::wants_open(question) {
        return None;
    }
    let tokens = words(question);
    let at = tokens.iter().position(|word| {
        matches!(
            word.as_str(),
            "open"
                | "launch"
                | "start"
                | "открой"
                | "откройте"
                | "открыть"
                | "запусти"
                | "запустите"
                | "запустить"
        )
    })?;
    if tokens[..at].iter().any(|word| {
        !matches!(
            word.as_str(),
            "please"
                | "mate"
                | "can"
                | "could"
                | "you"
                | "пожалуйста"
                | "можешь"
                | "можете"
                | "ты"
                | "вы"
        )
    }) {
        return None;
    }
    let target: Vec<_> = tokens[at + 1..]
        .iter()
        .filter(|word| {
            !matches!(
                word.as_str(),
                "please" | "пожалуйста" | "the" | "a" | "my" | "мне" | "мой" | "up"
            )
        })
        .cloned()
        .collect();
    if target.is_empty()
        || target.iter().any(|word| {
            matches!(
                word.as_str(),
                "this"
                    | "that"
                    | "it"
                    | "here"
                    | "button"
                    | "menu"
                    | "tab"
                    | "screen"
                    | "это"
                    | "этот"
                    | "эту"
                    | "здесь"
                    | "кнопку"
                    | "меню"
                    | "вкладку"
                    | "экране"
            )
        })
    {
        return None;
    }
    if remote_access(&target.join(" ")).is_some() {
        return Some(target.join(" "));
    }
    // Explicit sites keep the existing validated website route.
    let address = question.split_whitespace().any(|word| {
        word.trim_matches(|ch: char| !ch.is_alphanumeric())
            .contains('.')
    });
    if address
        || target.iter().any(|word| {
            matches!(
                word.as_str(),
                "website"
                    | "site"
                    | "сайт"
                    | "youtube"
                    | "ютуб"
                    | "google"
                    | "гугл"
                    | "bing"
                    | "yandex"
                    | "яндекс"
                    | "wikipedia"
                    | "википедию"
                    | "instagram"
                    | "facebook"
                    | "netflix"
            )
        })
    {
        return None;
    }
    // Keep the fixed allowlist's existing route, including Settings subpages.
    if target.len() == 1
        && !question.contains('+')
        && (crate::matescript::APPS
            .iter()
            .any(|(id, _, _, _)| id.words().iter().any(|alias| target[0] == *alias))
            || matches!(
                target[0].as_str(),
                "калькулятор"
                    | "блокнот"
                    | "проводник"
                    | "камеру"
                    | "камера"
                    | "фотографии"
                    | "часы"
                    | "будильник"
                    | "почту"
                    | "почта"
                    | "календарь"
                    | "магазин"
                    | "браузер"
                    | "интернет"
            ))
    {
        return None;
    }
    if target == ["file", "explorer"] || target == ["microsoft", "store"] {
        return None;
    }
    if target
        .iter()
        .any(|word| matches!(word.as_str(), "settings" | "настройки" | "параметры"))
    {
        return None;
    }
    Some(target.join(" "))
}

fn contains(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|part| part == needle)
}

const REMOTE: &[&str] = &[
    "AnyDesk",
    "TeamViewer",
    "RustDesk",
    "UltraViewer",
    "Quick Assist",
    "Быстрая помощь",
    "Remote Desktop Connection",
    "Подключение к удалённому рабочему столу",
    "mstsc",
    "Chrome Remote Desktop",
    "RemotePC",
    "Splashtop",
    "LogMeIn",
    "GoToMyPC",
    "Supremo",
    "AeroAdmin",
    "Ammyy",
    "ConnectWise",
    "ScreenConnect",
    "Zoho Assist",
];

/// A warning is available even when the remote-access program is not installed.
pub fn remote_access(name: &str) -> Option<&'static str> {
    let name = words(name);
    REMOTE
        .iter()
        .copied()
        .find(|alias| contains(&name, &words(alias)))
}

/// Folder components and display names are checked separately from shell paths.
pub fn blocked(name: &str, folders: &[&str]) -> bool {
    let lower = name.to_lowercase();
    remote_access(name).is_some()
        || folders.iter().any(|folder| {
            matches!(
                folder.to_lowercase().as_str(),
                "startup"
                    | "автозагрузка"
                    | "windows tools"
                    | "administrative tools"
                    | "средства windows"
                    | "администрирование"
            )
        })
        || [
            "uninstall",
            "удалить",
            "remove",
            "reset",
            "format",
            "recovery",
            "диск",
            "disk",
        ]
        .iter()
        .any(|part| lower.contains(part))
        || [
            "cmd",
            "Command Prompt",
            "PowerShell",
            "Windows Terminal",
            "Registry Editor",
            "regedit",
            "Task Scheduler",
            "Computer Management",
            "Services",
            "Group Policy",
            "Run",
            "командная строка",
            "терминал",
            "редактор реестра",
            "планировщик заданий",
            "управление компьютером",
            "службы",
            "групповая политика",
            "выполнить",
        ]
        .iter()
        .any(|alias| contains(&words(name), &words(alias)))
}

/// Longest whole-word name wins; identical best names at different paths are a tie.
pub fn find<'a>(target: &str, names: impl IntoIterator<Item = &'a str>) -> Match {
    let target = words(target);
    let mut best = Match::None;
    let mut score = 0;
    for (index, name) in names.into_iter().enumerate() {
        if blocked(name, &[]) {
            continue;
        }
        let name = words(name);
        if !contains(&name, &target) && !contains(&target, &name) {
            continue;
        }
        let rank = if name == target {
            usize::MAX
        } else {
            name.len().min(target.len())
        };
        if rank > score {
            score = rank;
            best = Match::One(index);
        } else if rank == score {
            best = Match::Ambiguous;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_remote_access_name_is_refused_but_family_calls_are_allowed() {
        for name in REMOTE.iter().copied().chain(["энидеск", "тимвьювер"]) {
            assert!(remote_access(name).is_some(), "{name}");
            assert!(blocked(name, &[]), "{name}");
            assert_eq!(find(name, [name]), Match::None);
            assert!(requested_app(&format!("Open {name}")).is_some(), "{name}");
        }
        for name in ["Zoom", "Skype", "Teams", "Webex", "WhatsApp", "Telegram"] {
            assert!(!blocked(name, &[]), "{name}");
        }
    }

    #[test]
    fn admin_destructive_and_startup_entries_are_never_offered() {
        for name in [
            "cmd",
            "Command Prompt",
            "PowerShell 7",
            "Windows Terminal",
            "Registry Editor",
            "regedit",
            "Task Scheduler",
            "Computer Management",
            "Services",
            "Group Policy",
            "Run",
            "Uninstall Telegram",
            "Удалить программу",
            "Remove App",
            "Reset",
            "Format USB",
            "Recovery",
            "Disk Management",
            "Управление дисками",
        ] {
            assert!(blocked(name, &[]), "{name}");
        }
        for folder in [
            "Startup",
            "Windows Tools",
            "Administrative Tools",
            "Автозагрузка",
        ] {
            assert!(blocked("Telegram", &[folder]));
        }
    }

    #[test]
    fn matches_whole_words_transliterates_and_refuses_ties() {
        for (ru, en) in [
            ("телеграм", "Telegram"),
            ("ворд", "Word"),
            ("эксель", "Excel"),
            ("зум", "Zoom"),
            ("скайп", "Skype"),
            ("вотсап", "WhatsApp"),
            ("ватсап", "WhatsApp"),
            ("вайбер", "Viber"),
            ("хром", "Chrome"),
        ] {
            assert_eq!(find(ru, [en]), Match::One(0));
        }
        assert_eq!(find("Word", ["WordPad"]), Match::None);
        assert_eq!(find("word", ["WORD"]), Match::One(0));
        assert_eq!(find("", ["Word"]), Match::None);
        assert_eq!(find("Word", ["Microsoft Word", "Word"]), Match::One(1));
        assert_eq!(
            find("Telegram", ["Telegram Desktop", "Telegram Web"]),
            Match::Ambiguous
        );
        assert_eq!(find("Telegram", ["Telegram", "Telegram"]), Match::Ambiguous);
        assert_eq!(
            find("Telegram Web", ["Telegram Desktop", "Telegram Web"]),
            Match::One(1)
        );
    }

    #[test]
    fn requests_use_only_words_after_a_positive_open_verb() {
        assert_eq!(
            requested_app("Could you open Telegram please?"),
            Some("telegram".into())
        );
        assert_eq!(
            requested_app("Открой, пожалуйста, телеграм"),
            Some("telegram".into())
        );
        assert_eq!(requested_app("Open Telegram."), Some("telegram".into()));
        assert_eq!(requested_app("Открой зум."), Some("zoom".into()));
        assert_eq!(requested_app("Open Photoshop"), Some("photoshop".into()));
        assert_eq!(
            requested_app("Open Notepad Next"),
            Some("notepad next".into())
        );
        assert_eq!(requested_app("Open Notepad++"), Some("notepad".into()));
        for question in [
            "Don't open Telegram",
            "Never launch Telegram",
            "Telegram is great",
            "Open this button",
            "How do I open Telegram?",
            "Open calculator",
            "Open settings sound",
            "Open sound settings",
            "Открой камеру",
            "Открой настройки звука",
            "Open File Explorer",
            "Open example.com",
            "Open https://example.com",
        ] {
            assert_eq!(requested_app(question), None, "{question}");
        }
    }

    #[test]
    fn unknown_apps_and_remote_access_stay_chat_without_screen_search() {
        use crate::api::intent::{Intent, guess_intent};
        for question in [
            "Open Telegram",
            "Открой зум",
            "Open UnknownApp",
            "Открой неизвестную программу",
            "Open Photoshop",
            "Open Chrome Remote Desktop",
            "Open энидеск",
            "Open тимвьювер",
            "Open Telegram.",
            "Открой зум.",
        ] {
            assert_eq!(guess_intent(question), Intent::Chat, "{question}");
        }
    }
}
