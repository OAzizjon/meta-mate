//! Mate's lesson card (Muse Glimmer + Muse Image): when a lesson ends well, a keepsake of
//! its steps that an older person can keep or print. The proxy's `/v1/glimmer` words it
//! (the question and the spoken steps in simpler words of the person's language, and an
//! English scene), `/v1/image` draws the scene with no text in it (image models garble
//! lettering), the PNG goes to `Pictures\MetaMate` and the `card` window (card.html) shows
//! it beside the title and steps. Without Glimmer's wording the card keeps the lesson's own
//! words and a generic scene. It all runs beside the lesson, never in its way: a failure is
//! logged (never with the person's words) and nothing is shown.
//!
//! The page gets `lesson-card` with `{ title, steps, imagePath }`, where `imagePath` is the
//! picture as a `data:` URL: the WebView needs no asset protocol and no file scope. A new
//! window's page does not listen yet, so the card is sent once the page emits `card-ready`
//! (or after 3 s, in case that event was lost).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use mm_core::constants::{APP_NAME, ROBOT_NAME};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Listener, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

use crate::proxy::Proxy;

/// The card window's label and page, the event its page listens to and the one it sends
/// once it listens.
const WINDOW: &str = "card";
const PAGE: &str = "card.html";
const EVENT: &str = "lesson-card";
const READY_EVENT: &str = "card-ready";
/// The most steps a card shows (teacher.rs keeps no more).
pub(crate) const MAX_STEPS: usize = 6;
/// The card's lines: with these caps Glimmer's message stays far under the proxy's 4000
/// characters.
const TITLE_CHARS: usize = 80;
const STEP_CHARS: usize = 150;
/// With a scene this long at most, the prompt stays under the proxy's limit
/// (`PROMPT_CHARS`) uncut.
const SCENE_CHARS: usize = 300;
const PROMPT_CHARS: usize = 1500;
/// Glimmer's reply: a title, the steps and a scene as JSON (room for six long Russian
/// steps and the model's short reasoning).
const WORDING_TOKENS: u32 = 600;
/// The picture when Glimmer wrote no scene.
const GENERIC_SCENE: &str =
    "a person at a computer smiling, with a small glowing light-bulb helper beside them";
/// The title's share of the file name.
const FILE_TITLE_CHARS: usize = 40;
/// Two cards with the same title in one minute: `name (2).png` and so on, up to this.
const SAME_NAME_LIMIT: u32 = 50;
const READY_LIMIT: Duration = Duration::from_secs(3);

/// What the page shows: the title and steps (Glimmer's, else the lesson's own) and the
/// picture (a `data:` URL; the page takes a path or a URL there), in the app's language
/// (`en` or `ru`).
#[derive(Serialize)]
struct LessonCard {
    title: String,
    steps: Vec<String>,
    #[serde(rename = "imagePath")]
    image: String,
    locale: String,
}

/// Glimmer's wording of a card: the title and steps in the person's language, the
/// picture's scene in English. The person's words: no `Debug` outside tests, so never a log.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Wording {
    title: String,
    steps: Vec<String>,
    scene: String,
}

/// Words, draws, saves and shows the card of a lesson that ended well: `question` titles
/// it, `steps` are the lesson's commentary lines in order (the first `MAX_STEPS` are kept).
pub(crate) async fn make(app: AppHandle, question: String, steps: Vec<String>) {
    let ended = SystemTime::now();
    let title = one_line(&question, TITLE_CHARS);
    let steps: Vec<String> = steps
        .iter()
        .map(|step| one_line(&without_tags(step), STEP_CHARS))
        .filter(|step| !step.is_empty())
        .take(MAX_STEPS)
        .collect();
    if title.is_empty() || steps.is_empty() {
        eprintln!("card: nothing to draw");
        return;
    }
    let Some(proxy) = app.try_state::<Proxy>().map(|proxy| proxy.inner().clone()) else {
        eprintln!("card: no proxy");
        return;
    };
    let (title, steps, scene) = match ask_wording(&proxy, &title, &steps).await {
        Some(wording) => (wording.title, wording.steps, wording.scene),
        None => (title, steps, GENERIC_SCENE.to_owned()),
    };
    let png = match proxy.image(&prompt(&scene)).await {
        Ok(png) => png,
        Err(error) => {
            eprintln!("card: image: {error}");
            return;
        }
    };
    let folder = match app.path().picture_dir() {
        Ok(pictures) => pictures.join(APP_NAME),
        Err(error) => {
            eprintln!("card: pictures folder: {error}");
            return;
        }
    };
    let stem = file_stem(ended, &title);
    let image = data_url(&png);
    match tokio::task::spawn_blocking(move || save(&folder, &stem, &png)).await {
        Ok(Ok(_)) => {}
        // io errors name no path, so no user name.
        Ok(Err(error)) => {
            eprintln!("card: save: {error}");
            return;
        }
        Err(_) => {
            eprintln!("card: save: worker failed");
            return;
        }
    }
    let locale = app
        .try_state::<crate::answer_stream::AnswerStream>()
        .map_or_else(|| "en".into(), |stream| stream.locale());
    show(
        &app,
        &LessonCard {
            title,
            steps,
            image,
            locale,
        },
    )
    .await;
}

fn data_url(png: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

/// Glimmer's wording for the card, or None when it failed or wrote no usable card (logged,
/// never with the person's words).
async fn ask_wording(proxy: &Proxy, title: &str, steps: &[String]) -> Option<Wording> {
    let reply = match proxy
        .glimmer(
            &wording_system(),
            &wording_request(title, steps),
            WORDING_TOKENS,
        )
        .await
    {
        Ok(reply) => reply,
        Err(error) => {
            eprintln!("card: wording: {error}");
            return None;
        }
    };
    // More steps than the lesson had would be invented ones.
    let wording = parse_wording(&reply, steps.len());
    if wording.is_none() {
        eprintln!("card: wording: not a usable card");
    }
    wording
}

/// Glimmer's instructions (English; the reply keeps the person's language).
fn wording_system() -> String {
    format!(
        "You write a keepsake card for an older person who just learned something on their \
         computer. Reply with JSON only: {{\"title\": \"...\", \"steps\": [\"...\"], \
         \"scene\": \"...\"}}. Write \"title\" and \"steps\" in the SAME language as the \
         question (Russian stays Russian): short, simple everyday words, no jargon. At most \
         {MAX_STEPS} steps, each at most 90 characters; keep the order and meaning of the \
         lesson's steps and never invent steps. Write \"scene\" in English: one sentence \
         describing a warm illustration of the task's idea, with NO text, letters, numbers, \
         logos, screenshots or brand names in it."
    )
}

/// Glimmer's message: the question and the lesson's steps, numbered.
fn wording_request(title: &str, steps: &[String]) -> String {
    let numbered: Vec<String> = steps
        .iter()
        .enumerate()
        .map(|(index, step)| format!("{}. {step}", index + 1))
        .collect();
    format!("Question: {title}\nSteps:\n{}", numbered.join("\n"))
}

/// Glimmer's reply as a card's wording: the JSON object in it (fenced or with prose around:
/// the first `{` to the last `}`) with a title, 1..=`max_steps` steps, none empty, and an
/// English scene of at most `SCENE_CHARS`; None for anything else. Too many steps is None,
/// not a cut: the extra ones were split or invented, and a cut would drop the lesson's
/// last steps (often the one that finishes the task).
fn parse_wording(reply: &str, max_steps: usize) -> Option<Wording> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let value: Value = serde_json::from_str(reply.get(start..=end)?).ok()?;
    let title = one_line(value.get("title")?.as_str()?, TITLE_CHARS);
    let steps = value
        .get("steps")?
        .as_array()?
        .iter()
        .map(|step| step.as_str().map(|step| one_line(step, STEP_CHARS)))
        .collect::<Option<Vec<String>>>()?;
    let scene = one_line(value.get("scene")?.as_str()?, usize::MAX);
    // ASCII-ish: a Cyrillic (or any non-Latin) scene is not the English one asked for.
    let english = !scene.chars().any(|c| c.is_alphabetic() && !c.is_ascii());
    if title.is_empty()
        || steps.is_empty()
        || steps.len() > max_steps
        || steps.iter().any(String::is_empty)
        || scene.is_empty()
        || scene.chars().count() > SCENE_CHARS
        || !english
    {
        return None;
    }
    Some(Wording {
        title,
        steps,
        scene,
    })
}

/// The picture's prompt in English, with no text in the picture: image models garble
/// lettering, and the page shows the steps crisply in the person's language.
fn prompt(scene: &str) -> String {
    let stop = if scene.ends_with(['.', '!', '?']) {
        ""
    } else {
        "."
    };
    // One style card to card: soft gouache on a warm cream ground, Mate in a corner, flat
    // enough for a home printer (DeepSeek's pick of five, 2026-10-28).
    let prompt = format!(
        "A soft gouache illustration for an older person: {scene}{stop} Warm cream \
         background, gentle light, a cosy home. In a corner, a small glowing light-bulb \
         character named {ROBOT_NAME}, with a friendly face, a round body and short arms, \
         cheers them on. No text, no letters, no numbers, no logos, no screenshots, no brand \
         names. Large clear shapes, high contrast, prints well at small size."
    );
    prompt.chars().take(PROMPT_CHARS).collect()
}

/// A line without its MateScript tags: the teacher's lines may start with
/// `<lang en><mood happy>`.
pub(crate) fn without_tags(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        plain.push_str(&rest[..open]);
        match rest[open..].find('>') {
            Some(close) => rest = &rest[open + close + 1..],
            None => {
                rest = &rest[open..];
                break;
            }
        }
    }
    plain.push_str(rest);
    plain
}

/// `text` on one line: control characters (line breaks too) become spaces, runs of spaces
/// one; longer than `max` characters, it is cut to `max` with an ellipsis.
fn one_line(text: &str, max: usize) -> String {
    let spaced: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let line = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let cut: String = line.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// `<YYYY-MM-DD HH-MM> <short title>`, the name of a card made at `time` without its
/// extension; the stamp alone when nothing of the title can be in a file name.
fn file_stem(time: SystemTime, title: &str) -> String {
    let stamp = stamp(time);
    let short = file_title(title);
    if short.is_empty() {
        stamp
    } else {
        format!("{stamp} {short}")
    }
}

/// The title as a file name may hold it: the characters Windows refuses in names become
/// spaces, at most `FILE_TITLE_CHARS`, no dot or space at the end.
fn file_title(title: &str) -> String {
    let allowed: String = title
        .chars()
        .map(|c| if r#"<>:"/\|?*"#.contains(c) { ' ' } else { c })
        .collect();
    one_line(&allowed, FILE_TITLE_CHARS)
        .trim_end_matches(['.', ' '])
        .to_owned()
}

/// `YYYY-MM-DD HH-MM` of `time` in UTC: the standard library knows no time zone.
fn stamp(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (year, month, day) = civil(seconds / 86_400);
    let minutes = seconds % 86_400 / 60;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}-{:02}",
        minutes / 60,
        minutes % 60
    )
}

/// The calendar date `days` after 1970-01-01 (H. Hinnant's `civil_from_days`).
fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // Months counted from March, so the leap day ends the year.
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// Writes `png` as `<stem>.png` in `folder` (created when missing), or as `<stem> (2).png`
/// and so on when that name is taken: an older card is never overwritten.
fn save(folder: &Path, stem: &str, png: &[u8]) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    for copy in 1..=SAME_NAME_LIMIT {
        let name = if copy == 1 {
            format!("{stem}.png")
        } else {
            format!("{stem} ({copy}).png")
        };
        let path = folder.join(name);
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        if let Err(error) = file.write_all(png) {
            drop(file);
            // Half a picture is no keepsake.
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        return Ok(path);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "every name for this card is taken",
    ))
}

/// Opens the card window, or brings it back when it is open, and hands the card to its
/// page.
async fn show(app: &AppHandle, card: &LessonCard) {
    // Listening before the window exists: its page may be quick.
    let (ready, mut page_ready) = oneshot::channel();
    let listener = app.once_any(READY_EVENT, move |_| {
        let _ = ready.send(());
    });
    let was_open = match open(app) {
        Ok(was_open) => was_open,
        Err(error) => {
            eprintln!("card: window: {error}");
            app.unlisten(listener);
            return;
        }
    };
    if !was_open {
        // A picture is megabytes: sent once, when the page listens.
        let _ = tokio::time::timeout(READY_LIMIT, &mut page_ready).await;
    }
    send(app, card);
    app.unlisten(listener);
}

/// The card window (640x720, centred): built when missing, else unminimized and shown;
/// true when it was open already. Built on the async runtime (this task's), never in an
/// event handler (lib.rs `open_settings`). A normal window the person may click, move and
/// close, but nobody asked for it: it appears without the focus, and the person's app
/// keeps the keyboard (invariant 1).
fn open(app: &AppHandle) -> tauri::Result<bool> {
    if let Some(window) = app.get_webview_window(WINDOW) {
        // Tauri's unminimize and show activate the window: back without the focus.
        #[cfg(windows)]
        mm_win32::overlay::show(window.hwnd()?);
        #[cfg(not(windows))]
        window.show()?;
        return Ok(true);
    }
    let window = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App(PAGE.into()))
        .title(APP_NAME)
        .inner_size(640.0, 720.0)
        .center()
        .focused(false)
        .visible(false)
        .additional_browser_args(crate::overlay::BROWSER_ARGS)
        .build()?;
    // Built hidden and shown without activation, like a window brought back.
    #[cfg(windows)]
    mm_win32::overlay::show(window.hwnd()?);
    #[cfg(not(windows))]
    window.show()?;
    Ok(false)
}

fn send(app: &AppHandle, card: &LessonCard) {
    if let Err(error) = app.emit_to(WINDOW, EVENT, card) {
        eprintln!("card: event: {error}");
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    /// An empty folder of its own for each test (tests run in parallel).
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("metamate-card-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn stamps_are_utc_dates_and_minutes() {
        assert_eq!(stamp(at(0)), "1970-01-01 00-00");
        assert_eq!(stamp(at(951_868_800)), "2000-03-01 00-00");
        assert_eq!(stamp(at(1_709_251_140)), "2024-02-29 23-59");
        assert_eq!(stamp(at(1_793_213_820)), "2026-10-28 18-57");
        // Before 1970 (a broken clock) is the epoch.
        assert_eq!(
            stamp(UNIX_EPOCH - Duration::from_secs(60)),
            "1970-01-01 00-00"
        );
    }

    #[test]
    fn file_names_keep_the_title_without_what_windows_refuses() {
        let made = at(1_793_213_820);
        assert_eq!(
            file_stem(made, "How do I make the text bigger?"),
            "2026-10-28 18-57 How do I make the text bigger"
        );
        assert_eq!(
            file_stem(made, "Как увеличить шрифт?"),
            "2026-10-28 18-57 Как увеличить шрифт"
        );
        assert_eq!(file_title(r#"a<b>c:d"e/f\g|h?i*j"#), "a b c d e f g h i j");
        assert_eq!(file_title("Open\nC:\\Users\t..."), "Open C Users");
        assert_eq!(file_stem(made, " ?*. "), "2026-10-28 18-57");
        let long = file_title(&"word ".repeat(20));
        assert_eq!(long.chars().count(), FILE_TITLE_CHARS);
        assert!(long.ends_with('…'), "{long}");
    }

    #[test]
    fn the_teachers_tags_never_reach_the_card() {
        assert_eq!(
            without_tags("<lang en><mood happy>Open Settings."),
            "Open Settings."
        );
        assert_eq!(without_tags("Press 2 < 3 now"), "Press 2 < 3 now");
        assert_eq!(without_tags("Plain."), "Plain.");
    }

    #[test]
    fn lines_are_one_line_and_cut_with_an_ellipsis() {
        assert_eq!(one_line("  Open\r\nthe \t menu\u{7} ", 80), "Open the menu");
        assert_eq!(one_line("abcdef", 6), "abcdef");
        assert_eq!(one_line("abcd efgh", 6), "abcd…");
        assert_eq!(one_line("абвгдеж", 4), "абв…");
        assert_eq!(one_line(" \n ", 10), "");
    }

    fn wording(title: &str, steps: &[&str], scene: &str) -> Wording {
        Wording {
            title: title.to_owned(),
            steps: steps.iter().map(|step| (*step).to_owned()).collect(),
            scene: scene.to_owned(),
        }
    }

    /// A reply in Glimmer's shape.
    fn reply(title: &str, steps: Value, scene: &str) -> String {
        json!({"title": title, "steps": steps, "scene": scene}).to_string()
    }

    #[test]
    fn glimmer_gets_the_question_and_the_numbered_steps() {
        let steps = vec!["Open Settings.".to_owned(), "Нажмите «Экран».".to_owned()];
        assert_eq!(
            wording_request("Bigger text", &steps),
            "Question: Bigger text\nSteps:\n1. Open Settings.\n2. Нажмите «Экран»."
        );
        assert!(wording_system().contains("At most 6 steps"));
        // The proxy takes at most 4000 characters a message.
        let title = one_line(&"Ж".repeat(500), TITLE_CHARS);
        let steps = vec![one_line(&"щ".repeat(500), STEP_CHARS); MAX_STEPS];
        assert!(wording_request(&title, &steps).chars().count() <= 4000);
        assert!(wording_system().chars().count() <= 4000);
    }

    #[test]
    fn glimmer_wording_is_found_plain_fenced_or_among_prose() {
        let plain = reply(
            "Крупный шрифт",
            json!(["Откройте «Параметры».", "Нажмите «Экран»."]),
            "An old woman smiles at her laptop.",
        );
        let expected = Some(wording(
            "Крупный шрифт",
            &["Откройте «Параметры».", "Нажмите «Экран»."],
            "An old woman smiles at her laptop.",
        ));
        assert_eq!(parse_wording(&plain, 2), expected);
        assert_eq!(
            parse_wording(&format!("```json\n{plain}\n```"), 2),
            expected
        );
        assert_eq!(
            parse_wording(&format!("Here is the card:\n{plain}\nI hope it helps!"), 2),
            expected
        );
    }

    #[test]
    fn glimmer_lines_are_one_line_and_cut() {
        let long = format!("Open\tSettings {}", "and more ".repeat(30));
        let wording = parse_wording(
            &reply(
                "  Bigger\ntext ",
                json!([long]),
                "An old woman smiles\nat her laptop.",
            ),
            1,
        )
        .unwrap();
        assert_eq!(wording.title, "Bigger text");
        assert!(wording.steps[0].starts_with("Open Settings and more"));
        assert!(wording.steps[0].chars().count() <= STEP_CHARS);
        assert!(wording.steps[0].ends_with('…'), "{}", wording.steps[0]);
        assert_eq!(wording.scene, "An old woman smiles at her laptop.");
    }

    #[test]
    fn a_glimmer_reply_that_is_no_good_card_is_none() {
        let scene = "An old man smiles.";
        // More steps than the lesson had: the extra ones were invented.
        let three = json!(["One.", "Two.", "Three."]);
        assert_eq!(parse_wording(&reply("Bigger text", three, scene), 2), None);
        assert_eq!(
            parse_wording(&reply("Bigger text", json!([]), scene), 2),
            None
        );
        let empty_step = json!(["One.", " \n "]);
        assert_eq!(
            parse_wording(&reply("Bigger text", empty_step, scene), 2),
            None
        );
        let number_step = json!(["One.", 2]);
        assert_eq!(
            parse_wording(&reply("Bigger text", number_step, scene), 2),
            None
        );
        assert_eq!(
            parse_wording(&reply(" \t ", json!(["One."]), scene), 2),
            None
        );
        // The scene: English, one sentence, not empty, not too long.
        let russian = "Мужчина улыбается.";
        assert_eq!(
            parse_wording(&reply("Bigger text", json!(["One."]), russian), 2),
            None
        );
        assert_eq!(
            parse_wording(&reply("Bigger text", json!(["One."]), " "), 2),
            None
        );
        let long = "a".repeat(SCENE_CHARS + 1);
        assert_eq!(
            parse_wording(&reply("Bigger text", json!(["One."]), &long), 2),
            None
        );
        // Missing or misshapen fields.
        assert_eq!(
            parse_wording(r#"{"title": "Bigger text", "steps": ["One."]}"#, 2),
            None
        );
        assert_eq!(
            parse_wording(r#"{"steps": ["One."], "scene": "A man."}"#, 2),
            None
        );
        assert_eq!(
            parse_wording(r#"{"title": "Bigger text", "scene": "A man."}"#, 2),
            None
        );
        let one_string = r#"{"title": "Bigger text", "steps": "One.", "scene": "A man."}"#;
        assert_eq!(parse_wording(one_string, 2), None);
        // Not JSON, or not an object.
        assert_eq!(parse_wording("Sorry, I cannot help with that.", 2), None);
        assert_eq!(parse_wording("{title: Bigger text}", 2), None);
        assert_eq!(parse_wording("} and {", 2), None);
        assert_eq!(parse_wording(r#"["Bigger text"]"#, 2), None);
    }

    #[test]
    fn the_prompt_draws_the_scene_and_mate_without_text() {
        assert_eq!(
            prompt("An old woman smiles at her laptop."),
            "A soft gouache illustration for an older person: An old woman smiles at her \
             laptop. Warm cream background, gentle light, a cosy home. In a corner, a small \
             glowing light-bulb character named Mate, with a friendly face, a round body and \
             short arms, cheers them on. No text, no letters, no numbers, no logos, no \
             screenshots, no brand names. Large clear shapes, high contrast, prints well at \
             small size."
        );
        // A scene without its full stop gets one.
        assert!(prompt(GENERIC_SCENE).contains("beside them. Warm cream background"));
    }

    #[test]
    fn the_longest_prompt_fits_the_proxy_uncut() {
        let scene = "w".repeat(SCENE_CHARS);
        assert!(parse_wording(&reply("Bigger text", json!(["One."]), &scene), 1).is_some());
        let prompt = prompt(&scene);
        assert!(prompt.chars().count() <= PROMPT_CHARS);
        assert!(prompt.ends_with("prints well at small size."), "{prompt}");
    }

    #[test]
    fn a_second_card_of_the_same_minute_gets_its_own_file() {
        let dir = folder("save");
        let first = save(&dir, "2026-10-28 18-57 Bigger text", b"one").unwrap();
        let second = save(&dir, "2026-10-28 18-57 Bigger text", b"two").unwrap();
        assert_eq!(first, dir.join("2026-10-28 18-57 Bigger text.png"));
        assert_eq!(second, dir.join("2026-10-28 18-57 Bigger text (2).png"));
        assert_eq!(fs::read(&first).unwrap(), b"one");
        assert_eq!(fs::read(&second).unwrap(), b"two");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_card_has_the_shape_the_page_expects() {
        let card = LessonCard {
            title: "Bigger text".into(),
            steps: vec!["Open Settings.".into()],
            image: data_url(b"\x89PNG"),
            locale: "ru".into(),
        };
        assert_eq!(
            serde_json::to_value(&card).ok(),
            Some(json!({
                "title": "Bigger text",
                "steps": ["Open Settings."],
                "imagePath": "data:image/png;base64,iVBORw==",
                "locale": "ru"
            }))
        );
    }
}
