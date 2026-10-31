use serde_json::json;
use std::fs;

use super::*;

#[test]
fn captioned_images_require_png_and_the_byte_cap() {
    let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    assert!(notes::decode_png(&encode(b"not png")).is_err());
    assert!(notes::decode_png("not base64!").is_err());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.resize(PNG_MAX_BYTES, 0);
    assert_eq!(
        notes::decode_png(&encode(&png)).unwrap().len(),
        PNG_MAX_BYTES
    );
    png.push(0);
    assert!(notes::decode_png(&encode(&png)).is_err());
}

#[test]
fn a_text_only_card_retains_the_steps_in_the_page_payload() {
    let card = LessonCard {
        id: "text-card".into(),
        title: "Bigger text".into(),
        steps: vec!["Open Settings.".into()],
        image: String::new(),
        locale: "en".into(),
        words_by: WordsBy::Lesson,
    };
    let pending = PendingCard::default();
    *pending.0.lock().unwrap() = Some(card);
    // Reading never consumes the pending card: slow loads and reloads can ask again.
    for _ in 0..2 {
        let guard = pending.0.lock().unwrap();
        let value = serde_json::to_value(guard.as_ref().unwrap()).unwrap();
        assert_eq!(value["steps"], json!(["Open Settings."]));
        assert_eq!(value["imagePath"], "");
    }
}

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
         laptop. Warm cream background, gentle light, a cosy home. In a corner, exactly \
         one small glowing light-bulb character named Mate, with a friendly face, a round \
         body and short arms, cheers them on. No text, no letters, no numbers, no logos, \
         no screenshots, no brand names. Large clear shapes, high contrast, prints well at \
         small size."
    );
    // A scene without its full stop gets one.
    assert!(prompt(GENERIC_SCENE).contains("finish a task. Warm cream background"));
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
    let first = notes::save(&dir, "Bigger text", saved_card(1), None).unwrap();
    let second = notes::save(&dir, "Bigger text", saved_card(2), None).unwrap();
    assert_eq!(first.id, "Bigger text");
    assert_eq!(second.id, "Bigger text (2)");
    assert_eq!(notes::load(&dir, &first.id).unwrap().created, 1);
    assert_eq!(notes::load(&dir, &second.id).unwrap().created, 2);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_card_has_the_shape_the_page_expects() {
    let card = LessonCard {
        id: "Bigger text".into(),
        title: "Bigger text".into(),
        steps: vec!["Open Settings.".into()],
        image: data_url(b"\x89PNG"),
        locale: "ru".into(),
        words_by: WordsBy::Glimmer,
    };
    assert_eq!(
        serde_json::to_value(&card).ok(),
        Some(json!({
            "id": "Bigger text",
            "title": "Bigger text",
            "steps": ["Open Settings."],
            "imagePath": "data:image/png;base64,iVBORw==",
            "locale": "ru",
            "wordsBy": "glimmer"
        }))
    );
    assert_eq!(
        serde_json::to_value(WordsBy::Lesson).ok(),
        Some(json!("lesson"))
    );
}

fn saved_card(created: u64) -> notes::SavedCard {
    notes::SavedCard {
        version: 1,
        id: String::new(),
        title: "Bigger text".into(),
        question: "How do I make text bigger?".into(),
        steps: vec!["Open Settings.".into()],
        created,
        image: None,
    }
}

#[test]
fn sidecars_round_trip_text_and_captioned_images() {
    let dir = folder("sidecars");
    let saved = notes::save(&dir, "text", saved_card(42), None).unwrap();
    let loaded = notes::load(&dir, &saved.id).unwrap();
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.question, "How do I make text bigger?");
    assert_eq!(loaded.steps, vec!["Open Settings."]);
    assert!(loaded.image.is_none());
    let png = b"\x89PNG\r\n\x1a\ncaptioned";
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    notes::save_captioned(&dir, &saved.id, &encoded).unwrap();
    notes::save_captioned(&dir, &saved.id, &encoded).unwrap();
    let loaded = notes::load(&dir, &saved.id).unwrap();
    assert_eq!(loaded.image.as_deref(), Some("text.png"));
    assert_eq!(fs::read(dir.join("text.png")).unwrap(), png);
    assert_eq!(
        notes::list(&dir)[0].thumb.as_deref(),
        Some(data_url(png).as_str())
    );
    assert!(notes::save_captioned(&dir, "missing", &encoded).is_err());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn traversal_and_foreign_sidecars_are_rejected() {
    let dir = folder("invalid");
    fs::create_dir_all(&dir).unwrap();
    for id in [
        "", "..", "../card", "a/b", "a\\b", "C:card", "a..b", "card.",
    ] {
        assert!(!notes::valid_id(id), "{id}");
        assert!(notes::load(&dir, id).is_err());
    }
    assert!(notes::valid_id("2026-11-01 Анна (2)"));
    fs::write(dir.join("foreign.json"), b"{}").unwrap();
    let mut foreign = saved_card(1);
    foreign.id = "foreign".into();
    foreign.image = Some("../elsewhere.png".into());
    fs::write(
        dir.join("foreign.json"),
        serde_json::to_vec(&foreign).unwrap(),
    )
    .unwrap();
    assert!(notes::list(&dir).is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn cards_list_is_newest_first_capped_and_omits_large_thumbnails() {
    let dir = folder("list");
    for created in 0..65 {
        notes::save(&dir, &format!("card-{created}"), saved_card(created), None).unwrap();
    }
    let mut card = notes::load(&dir, "card-64").unwrap();
    card.image = Some("card-64.png".into());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.resize(2 * 1024 * 1024 + 1, 0);
    fs::write(dir.join("card-64.png"), png).unwrap();
    fs::write(dir.join("card-64.json"), serde_json::to_vec(&card).unwrap()).unwrap();
    fs::write(dir.join("broken.json"), b"not json").unwrap();
    let cards = notes::list(&dir);
    assert_eq!(cards.len(), 60);
    assert_eq!(cards.first().unwrap().created, 64);
    assert_eq!(cards.last().unwrap().created, 5);
    assert!(cards[0].thumb.is_none());
    let _ = fs::remove_dir_all(dir);
}
