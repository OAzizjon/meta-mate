//! Validated website and fixed Windows app targets; no shell input comes from the model.

use serde::Serialize;

macro_rules! apps {
    ($($id:ident, $name:literal, $target:literal, $key:literal;)+) => {
        /// An app from the fixed allowlist.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum AppId { $($id,)+ }

        /// The single app allowlist: script name, shell target and locale key.
        pub const APPS: &[(AppId, &str, &str, &str)] = &[
            $((AppId::$id, $name, $target, $key),)+
        ];

        impl AppId {
            /// Fixed shell target and translated display name key.
            pub fn spec(self) -> (&'static str, &'static str) {
                match self {
                    $(Self::$id => ($target, $key),)+
                }
            }
        }
    };
}

apps! {
    Calculator, "calculator", "calc.exe", "open.app_calculator";
    Notepad, "notepad", "notepad.exe", "open.app_notepad";
    Paint, "paint", "mspaint.exe", "open.app_paint";
    Settings, "settings", "ms-settings:", "open.app_settings";
    Files, "files", "explorer.exe", "open.app_files";
    Camera, "camera", "microsoft.windows.camera:", "open.app_camera";
    Photos, "photos", "ms-photos:", "open.app_photos";
    Clock, "clock", "ms-clock:", "open.app_clock";
    Mail, "mail", "outlookmail:", "open.app_mail";
    Calendar, "calendar", "outlookcal:", "open.app_calendar";
    Store, "store", "ms-windows-store:", "open.app_store";
}

/// Russian spellings of common sites, as people say them (stem, host label).
const SITE_SPELLINGS: &[(&str, &str)] = &[
    ("ютуб", "youtube"),
    ("ютюб", "youtube"),
    ("ютьюб", "youtube"),
    ("гугл", "google"),
    ("яндекс", "yandex"),
    ("википеди", "wikipedia"),
    ("фейсбук", "facebook"),
    ("инстаграм", "instagram"),
    ("нетфликс", "netflix"),
    ("вконтакт", "vk"),
    ("одноклассник", "ok"),
    ("госуслуг", "gosuslugi"),
    ("авито", "avito"),
    ("озон", "ozon"),
    ("вайлдберриз", "wildberries"),
    ("телеграм", "telegram"),
    ("ватсап", "whatsapp"),
    ("бинг", "bing"),
];

/// The label that names a site: the one before the top-level domain, or before a short
/// second-level one (`co.uk`, `com.au`, `org.ru`).
fn registrable_label<'a>(labels: &[&'a str]) -> Option<&'a str> {
    let n = labels.len();
    if n < 2 {
        return None;
    }
    let second = labels[n - 2];
    // A short second level counts only under a two-letter country ending (co.uk, com.au):
    // youtube.com.xyz is a "com" site, not YouTube (Command Code #1478).
    if n >= 3
        && labels[n - 1].len() == 2
        && matches!(second, "co" | "com" | "org" | "net" | "gov" | "edu" | "ac")
    {
        Some(labels[n - 3])
    } else {
        Some(second)
    }
}

/// The real domains of sites people name often: when the name is one of these, the host
/// must be that domain or under it, so an imitation such as youtube.com.co is refused.
const KNOWN_SITES: &[(&str, &[&str])] = &[
    ("youtube", &["youtube.com", "youtu.be"]),
    ("google", &["google.com", "google.ru"]),
    ("yandex", &["yandex.ru", "yandex.com", "ya.ru"]),
    ("wikipedia", &["wikipedia.org"]),
    ("gosuslugi", &["gosuslugi.ru"]),
    ("vk", &["vk.com", "vk.ru"]),
    ("ok", &["ok.ru"]),
    ("facebook", &["facebook.com"]),
    ("instagram", &["instagram.com"]),
    ("netflix", &["netflix.com"]),
    ("avito", &["avito.ru"]),
    ("ozon", &["ozon.ru"]),
    ("wildberries", &["wildberries.ru"]),
    ("telegram", &["telegram.org"]),
    ("whatsapp", &["whatsapp.com"]),
    ("bing", &["bing.com"]),
    ("bbc", &["bbc.co.uk", "bbc.com"]),
];

/// Whether `host` may carry the site name `name`: any host for an unknown name, only the
/// real domains for a known one.
fn real_site(name: &str, host: &str) -> bool {
    KNOWN_SITES
        .iter()
        .find(|(known, _)| *known == name)
        .is_none_or(|(_, domains)| {
            domains
                .iter()
                .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
        })
}

impl AppId {
    /// Word starts (English and Russian) that name this app in a request.
    pub fn words(self) -> &'static [&'static str] {
        match self {
            Self::Calculator => &["calculator", "calc", "калькулятор"],
            Self::Notepad => &["notepad", "блокнот"],
            Self::Paint => &["paint", "пэйнт", "паинт", "пейнт"],
            Self::Settings => &["settings", "setting", "настройк", "параметр"],
            Self::Files => &["files", "file", "explorer", "проводник", "файл"],
            Self::Camera => &["camera", "камер"],
            Self::Photos => &["photos", "photo", "pictures", "фото", "снимк"],
            Self::Clock => &["clock", "alarm", "timer", "часы", "будильник", "таймер"],
            Self::Mail => &["mail", "email", "e-mail", "почт"],
            Self::Calendar => &["calendar", "календар"],
            Self::Store => &["store", "магазин"],
        }
    }
}

/// A validated target from an `<open>` tag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum OpenTarget {
    /// An ASCII HTTPS address with a public DNS host.
    Site(String),
    /// A fixed app identifier.
    App(AppId),
}

impl OpenTarget {
    /// Reject ambiguous URLs rather than normalizing shell input.
    pub fn parse(raw: &str) -> Option<Self> {
        if let Some(name) = raw.strip_prefix("app:") {
            return APPS
                .iter()
                .find_map(|&(id, script, _, _)| (name == script).then_some(Self::App(id)));
        }
        if raw.len() > 200
            || !raw.is_ascii()
            || raw.bytes().any(|b| {
                b.is_ascii_control() || b.is_ascii_whitespace() || b"\"'\\<>`".contains(&b)
            })
        {
            return None;
        }
        let rest = raw.strip_prefix("https://")?;
        let host = rest.split(['/', '?', '#']).next()?;
        let lower = host.to_ascii_lowercase();
        if !host.contains('.') || lower.ends_with(".localhost") {
            return None;
        }
        let labels: Vec<_> = lower.split('.').collect();
        if labels.iter().any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || label.starts_with("xn--")
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) || !labels.last()?.bytes().all(|b| b.is_ascii_alphabetic())
        {
            return None;
        }
        Some(Self::Site(raw.into()))
    }

    /// Host shown to the person, without a path or query.
    /// Whether the person's `request` names this target: an app by one of its English or
    /// Russian words (word starts, so Russian endings match), a site by a label of its host
    /// (not `www`, not the top-level domain) or a common Russian spelling of it. A model
    /// answer that opens something the person did not name is dropped (Meta #1463).
    pub fn named_in(&self, request: &str) -> bool {
        let lower = request.to_lowercase();
        let words: Vec<&str> = lower
            .split(|ch: char| !(ch.is_alphanumeric() || ch == '.' || ch == '-'))
            .map(|word| word.trim_matches('.'))
            .filter(|word| !word.is_empty())
            .collect();
        let starts = |stems: &[&str]| {
            words
                .iter()
                .any(|word| stems.iter().any(|stem| word.starts_with(stem)))
        };
        match self {
            Self::App(id) => starts(id.words()),
            Self::Site(url) => {
                let Some(host) = self.host() else {
                    return false;
                };
                let host = host.to_ascii_lowercase();
                // Only a home page, unless the person said the whole address: a model-picked
                // path or query on a named site is not what they asked for (Cursor #1465).
                let rest = &url["https://".len() + host.len()..];
                if !(rest.is_empty() || rest == "/") && !lower.contains(&url.to_lowercase()) {
                    return false;
                }
                // The site's own name (the registrable label: youtube in www.youtube.com,
                // bbc in bbc.co.uk), never any label: youtube.com.evil.example is not
                // YouTube (Cursor #1465).
                let labels: Vec<&str> = host.split('.').collect();
                let Some(name) = registrable_label(&labels) else {
                    return false;
                };
                if !real_site(name, &host) {
                    return false;
                }
                words.iter().any(|word| {
                    *word == host
                        || word.strip_prefix("www.") == Some(host.as_str())
                        || *word == name
                        || (word.split('.').next() == Some(name)
                            && host.ends_with(word.trim_start_matches("www.")))
                }) || SITE_SPELLINGS
                    .iter()
                    .any(|(ru, site)| *site == name && starts(&[ru]))
            }
        }
    }

    pub fn host(&self) -> Option<&str> {
        match self {
            Self::Site(url) => url.strip_prefix("https://")?.split(['/', '?', '#']).next(),
            Self::App(_) => None,
        }
    }
}
