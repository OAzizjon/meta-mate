//! Mate's lesson card (Muse Glimmer + Muse Image): when a lesson ends well, a keepsake of
//! its steps that an older person can keep or print. The proxy's `/v1/glimmer` words it
//! (the question and the spoken steps in simpler words of the person's language, and an
//! English scene), `/v1/image` draws the scene with no text in it (image models garble
//! lettering), the PNG goes to `Pictures\MetaMate` and the `card` window (card.html) shows
//! it beside the title and steps. Without Glimmer's wording the card keeps the lesson's own
//! words and a generic scene. It all runs beside the lesson, never in its way: a failure is
//! logged (never with the person's words); image failures leave a printable text card.
//!
//! The page gets `lesson-card` with `{ id, title, steps, imagePath, locale, wordsBy }`, where
//! `imagePath` is the picture as a `data:` URL (the WebView needs no asset protocol and no
//! file scope) and `wordsBy` (`glimmer` or `lesson`) tells whose words the steps are, for
//! the card's credit line. A new window's page does not listen yet, so the card is sent
//! on `card-ready`, from managed state retained even when the page loads slowly.

use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use mm_core::constants::{APP_NAME, ROBOT_NAME};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Listener, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::proxy::Proxy;

#[path = "card_notes.rs"]
mod notes;
pub use notes::CardNote;

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
/// The picture when Glimmer wrote no scene. Never a helper character: the prompt adds Mate,
/// and a scene with one drew two (2026-10-28).
const GENERIC_SCENE: &str = "an older person at a computer at home, smiling as they finish a task";
/// The title's share of the file name.
const FILE_TITLE_CHARS: usize = 40;
/// Two cards with the same title in one minute: `name (2).png` and so on, up to this.
const SAME_NAME_LIMIT: u32 = 50;
const WORDING_LIMIT: Duration = Duration::from_secs(25);
const IMAGE_LIMIT: Duration = Duration::from_secs(40);
const PNG_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct PendingCard(Mutex<Option<LessonCard>>);

/// The page requests the retained card after installing its listener, including reloads.
pub(crate) fn init(app: &AppHandle) {
    let handle = app.clone();
    app.listen_any(READY_EVENT, move |_| {
        let pending = handle.state::<PendingCard>();
        let card = pending.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(card) = card.as_ref() {
            send(&handle, card);
        }
    });
}

/// What the page shows: the title and steps (Glimmer's, else the lesson's own, as
/// `words_by` says) and the picture (a `data:` URL; the page takes a path or a URL there),
/// in the app's language (`en` or `ru`).
#[derive(Serialize)]
struct LessonCard {
    id: String,
    title: String,
    steps: Vec<String>,
    #[serde(rename = "imagePath")]
    image: String,
    locale: String,
    #[serde(rename = "wordsBy")]
    words_by: WordsBy,
}

/// Whose words a card's title and steps are: the page credits Muse Glimmer for its own.
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum WordsBy {
    Glimmer,
    Lesson,
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
    let (title, steps, scene, words_by) = match ask_wording(&proxy, &title, &steps).await {
        Some(wording) => (
            wording.title,
            wording.steps,
            wording.scene,
            WordsBy::Glimmer,
        ),
        None => (title, steps, GENERIC_SCENE.to_owned(), WordsBy::Lesson),
    };
    let png = match tokio::time::timeout(IMAGE_LIMIT, proxy.image(&prompt(&scene))).await {
        Ok(Ok(png)) if png.len() <= PNG_MAX_BYTES => Some(png),
        Ok(Ok(_)) => {
            eprintln!("card: image: too large");
            None
        }
        Ok(Err(error)) => {
            eprintln!("card: image: {error}");
            None
        }
        Err(_) => {
            eprintln!("card: image: timed out");
            None
        }
    };
    let mut id = String::new();
    let mut image = String::new();
    if let Ok(pictures) = app.path().picture_dir() {
        let stem = file_stem(local(ended), &title);
        let card = notes::SavedCard {
            version: 1,
            id: String::new(),
            title: title.clone(),
            question,
            steps: steps.clone(),
            created: ended
                .duration_since(UNIX_EPOCH)
                .map_or(0, |time| time.as_secs()),
            image: None,
        };
        let saved = tokio::task::spawn_blocking(move || {
            let folder = pictures.join(APP_NAME);
            let card = notes::save(&folder, &stem, card, png.as_deref())?;
            let image = notes::image(&folder, &card, PNG_MAX_BYTES).unwrap_or_default();
            Ok::<_, io::Error>((card.id, image))
        })
        .await;
        if let Ok(Ok((saved_id, saved_image))) = saved {
            id = saved_id;
            image = saved_image;
        } else {
            eprintln!("card: could not save keepsake");
        }
    }
    let locale = app
        .try_state::<crate::answer_stream::AnswerStream>()
        .map_or_else(|| "en".into(), |stream| stream.locale());
    let _ = show(
        &app,
        LessonCard {
            id,
            title,
            steps,
            image,
            locale,
            words_by,
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
    let reply = match tokio::time::timeout(
        WORDING_LIMIT,
        proxy.glimmer(
            &wording_system(),
            &wording_request(title, steps),
            WORDING_TOKENS,
        ),
    )
    .await
    {
        Ok(Ok(reply)) => reply,
        Ok(Err(error)) => {
            eprintln!("card: wording: {error}");
            return None;
        }
        Err(_) => {
            eprintln!("card: wording: timed out");
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
         describing a warm illustration of the person and the task's idea, with NO text, \
         letters, numbers, logos, screenshots or brand names in it, and no helper or \
         mascot character (one is added to the picture separately)."
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
         background, gentle light, a cosy home. In a corner, exactly one small glowing \
         light-bulb character named {ROBOT_NAME}, with a friendly face, a round body and \
         short arms, cheers them on. No text, no letters, no numbers, no logos, no screenshots, no brand \
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

/// `time` on the local clock: the person reads the file name.
fn local(time: SystemTime) -> SystemTime {
    #[cfg(windows)]
    let offset = mm_win32::time::local_offset_seconds();
    #[cfg(not(windows))]
    let offset = 0;
    let shift = Duration::from_secs(offset.unsigned_abs());
    let shifted = if offset >= 0 {
        time.checked_add(shift)
    } else {
        time.checked_sub(shift)
    };
    shifted.unwrap_or(time)
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
        .replace("..", " ")
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

/// Opens the card window, or brings it back when it is open, and hands the card to its
/// page.
async fn show(app: &AppHandle, card: LessonCard) -> Result<(), String> {
    let pending = app.state::<PendingCard>();
    *pending.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(card);
    open(app).map_err(|_| card_error(app))?;
    let card = pending.0.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(card) = card.as_ref() {
        send(app, card);
    }
    Ok(())
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

fn card_locale(app: &AppHandle) -> String {
    app.try_state::<crate::answer_stream::AnswerStream>()
        .map_or_else(|| "en".into(), |stream| stream.locale())
}

fn card_error(app: &AppHandle) -> String {
    crate::locale_text(&card_locale(app), "card.operation_failed")
}

fn card_folder(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .picture_dir()
        .map(|path| path.join(APP_NAME))
        .map_err(|_| card_error(app))
}

#[tauri::command]
pub async fn cards_list(app: AppHandle) -> Vec<CardNote> {
    let Ok(folder) = card_folder(&app) else {
        return Vec::new();
    };
    tokio::task::spawn_blocking(move || notes::list(&folder))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn card_show(app: AppHandle, id: String) -> Result<(), String> {
    let folder = card_folder(&app)?;
    let loaded = tokio::task::spawn_blocking(move || {
        let card = notes::load(&folder, &id)?;
        let image = notes::image(&folder, &card, PNG_MAX_BYTES).unwrap_or_default();
        Ok::<_, io::Error>((card, image))
    })
    .await
    .map_err(|_| card_error(&app))?
    .map_err(|_| card_error(&app))?;
    let (card, image) = loaded;
    show(
        &app,
        LessonCard {
            id: card.id,
            title: card.title,
            steps: card.steps,
            image,
            locale: card_locale(&app),
            words_by: WordsBy::Lesson,
        },
    )
    .await
}

#[tauri::command]
pub async fn card_save_captioned(
    app: AppHandle,
    id: String,
    png_base64: String,
) -> Result<(), String> {
    let folder = card_folder(&app)?;
    tokio::task::spawn_blocking(move || notes::save_captioned(&folder, &id, &png_base64))
        .await
        .map_err(|_| card_error(&app))?
        .map_err(|_| card_error(&app))
}

#[tauri::command]
pub async fn cards_open_folder(app: AppHandle) -> Result<(), String> {
    let folder = card_folder(&app)?;
    let folder = tokio::task::spawn_blocking(move || notes::ensure_folder(&folder))
        .await
        .map_err(|_| card_error(&app))?
        .map_err(|_| card_error(&app))?;
    #[cfg(windows)]
    if mm_win32::shell::open_folder(&folder) {
        return Ok(());
    }
    #[cfg(not(windows))]
    let _ = folder;
    Err(card_error(&app))
}

/// `--card-selftest [ru]` (development builds): the card of a made-up lesson through the
/// dev proxy (Glimmer, then Muse Image), shown for a minute; then the app exits.
#[cfg(all(windows, debug_assertions))]
pub(crate) fn selftest(app: &AppHandle, russian: bool) {
    let proxy =
        crate::voice::proxy_config().and_then(|(base, invite)| Proxy::new(&base, &invite).ok());
    let Some(proxy) = proxy else {
        eprintln!("card: no proxy: start server/run_dev.py --tts-modal");
        app.exit(1);
        return;
    };
    crate::auth::restore(app, &proxy);
    app.manage(proxy);
    let (question, steps) = if russian {
        (
            "Как сделать текст на экране крупнее?",
            [
                "Откройте Параметры: это шестерёнка в меню Пуск.",
                "Нажмите «Специальные возможности».",
                "Выберите «Размер текста» и передвиньте ползунок вправо.",
                "Нажмите «Применить».",
            ],
        )
    } else {
        (
            "How do I make the text on my screen bigger?",
            [
                "Open Settings: it's the gear in the Start menu.",
                "Click Accessibility.",
                "Choose Text size and drag the slider to the right.",
                "Click Apply.",
            ],
        )
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        make(
            app.clone(),
            question.to_owned(),
            steps.iter().map(|step| (*step).to_owned()).collect(),
        )
        .await;
        eprintln!("card: selftest made the card in {:?}", started.elapsed());
        tokio::time::sleep(Duration::from_secs(60)).await;
        app.exit(0);
    });
}

#[cfg(test)]
#[path = "card_tests.rs"]
mod tests;
