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
    let spaced = words(name);
    // Glued forms too: "Any Desk", "Team-Viewer", "Quick Assist" and "QuickAssist" are one name.
    let glued = squash(name);
    REMOTE.iter().copied().find(|alias| {
        contains(&spaced, &words(alias)) || {
            let alias = squash(alias);
            alias.chars().count() >= 5 && glued.contains(&alias)
        }
    })
}

/// Lowercase letters and digits only, Cyrillic spellings of the remote tools in Latin.
fn squash(text: &str) -> String {
    let lower: String = text
        .to_lowercase()
        .replace('ё', "е")
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect();
    [
        ("энидеск", "anydesk"),
        ("энидэск", "anydesk"),
        ("анидеск", "anydesk"),
        ("anidesk", "anydesk"),
        ("тимвьювер", "teamviewer"),
        ("тимвьюер", "teamviewer"),
        ("тимвивер", "teamviewer"),
        ("растдеск", "rustdesk"),
        ("ультравьювер", "ultraviewer"),
        ("быстраяпомощь", "quickassist"),
    ]
    .iter()
    .fold(lower, |text, (from, to)| text.replace(from, to))
}

/// Program names inside a shortcut that are never opened, whatever the shortcut is called:
/// remote-access tools, shells and admin tools (Meta #1548 #1: the name alone can lie).
pub const BLOCKED_TARGETS: &[&str] = &[
    "anydesk",
    "teamviewer",
    "rustdesk",
    "ultraviewer",
    "quickassist",
    "mstsc",
    "remotepc",
    "splashtop",
    "logmein",
    "gotomypc",
    "supremo",
    "aeroadmin",
    "ammyy",
    "screenconnect",
    "connectwise",
    "zohoassist",
    "cmd.exe",
    "powershell",
    "pwsh",
    "wt.exe",
    "regedit",
    "mmc.exe",
    "taskschd",
    "compmgmt",
    "services.msc",
    "gpedit",
    "diskpart",
    "format.com",
];

/// Whether a shortcut file's bytes name a blocked program (the target path is stored as
/// ASCII and as UTF-16LE inside .lnk files).
pub fn shortcut_targets_blocked(bytes: &[u8]) -> bool {
    let ascii: String = bytes
        .iter()
        .map(|&b| (b as char).to_ascii_lowercase())
        .collect();
    let utf16: String = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            char::from_u32(u32::from(u16::from_le_bytes([pair[0], pair[1]]))).unwrap_or('\0')
        })
        .collect::<String>()
        .to_lowercase();
    let odd: String = bytes
        .get(1..)
        .unwrap_or_default()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            char::from_u32(u32::from(u16::from_le_bytes([pair[0], pair[1]]))).unwrap_or('\0')
        })
        .collect::<String>()
        .to_lowercase();
    BLOCKED_TARGETS
        .iter()
        .any(|name| ascii.contains(name) || utf16.contains(name) || odd.contains(name))
}

/// Folder components and display names are checked separately from shell paths.
pub fn blocked(name: &str, folders: &[&str]) -> bool {
    let lower = name.to_lowercase();
    remote_access(name).is_some()
        || folders.iter().any(|folder| {
            matches!(
                squash_folder(folder).as_str(),
                "startup"
                    | "автозагрузка"
                    | "windowstools"
                    | "administrativetools"
                    | "средстваwindows"
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

/// A folder name for the block list: lowercase letters and digits only ("Start-up " is "startup").
fn squash_folder(folder: &str) -> String {
    folder
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect()
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

    #[test]
    fn remote_tools_are_caught_spaced_glued_and_in_cyrillic() {
        for name in [
            "Any Desk",
            "Team-Viewer QS",
            "QuickAssist",
            "Quick Assist",
            "эни деск",
            "Тимвьювер",
            "Быстрая помощь",
        ] {
            assert!(super::remote_access(name).is_some(), "{name}");
        }
        for name in ["Telegram", "Zoom", "Skype", "Desktop Tools"] {
            assert!(super::remote_access(name).is_none(), "{name}");
        }
    }

    #[test]
    fn a_shortcut_pointing_at_a_blocked_program_is_refused_whatever_its_name() {
        let utf16 =
            |text: &str| -> Vec<u8> { text.encode_utf16().flat_map(u16::to_le_bytes).collect() };
        let mut lnk = vec![b'L', 0, 0, 0];
        lnk.extend(utf16(r"C:\Program Files\AnyDesk\AnyDesk.exe"));
        assert!(super::shortcut_targets_blocked(&lnk));
        let mut odd = vec![0u8];
        odd.extend(utf16(r"C:\Windows\System32\cmd.exe"));
        assert!(super::shortcut_targets_blocked(&odd));
        assert!(super::shortcut_targets_blocked(
            br"C:\WINDOWS\system32\WindowsPowerShell"
        ));
        let mut fine = vec![b'L', 0, 0, 0];
        fine.extend(utf16(r"C:\Program Files\Telegram Desktop\Telegram.exe"));
        assert!(!super::shortcut_targets_blocked(&fine));
    }

    #[test]
    fn startup_folders_are_caught_with_spaces_or_hyphens() {
        for folder in ["Startup ", "Start-up", "Автозагрузка ", "Windows-Tools"] {
            assert!(super::blocked("Some App", &[folder]), "{folder}");
        }
    }
}
