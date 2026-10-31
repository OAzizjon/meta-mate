//! The benchmark executable must use the app builder and image preparation unchanged.
#[allow(dead_code)]
#[path = "../examples/safety_request.rs"]
mod example;

use std::fs;
use std::io::Cursor;

use base64::{Engine, engine::general_purpose::STANDARD};
use mm_core::api::request::{ModelChoice, ReasoningEffort, render_element_list, safety_request};
use mm_core::geom::PhysRect;
use mm_core::redaction::{RedactionInput, redact};
use mm_core::shot::{Bgra, encode_png_rgb};

#[test]
fn example_matches_app_request_for_both_languages_models_and_downscale() {
    let path = std::env::temp_dir().join(format!("mm-safety-example-{}.png", std::process::id()));
    let rgb = [255, 0, 0, 0, 0, 255];
    fs::write(&path, encode_png_rgb(&rgb, 2, 1).unwrap()).unwrap();
    let pixels = [0, 0, 255, 255, 255, 0, 0, 255];
    let region = PhysRect::new(0, 0, 2, 1).unwrap();
    for (lang, locale) in [
        ("en", include_str!("../../../locales/en.json")),
        ("ru", include_str!("../../../locales/ru.json")),
    ] {
        let locale: serde_json::Value = serde_json::from_str(locale).unwrap();
        for (name, model) in [
            ("muse-spark-1.3", ModelChoice::Quick),
            ("muse-wonder-1.0", ModelChoice::Thorough),
        ] {
            let args: Vec<String> = [
                "--image",
                path.to_str().unwrap(),
                "--lang",
                lang,
                "--model",
                name,
                "--max-edge",
                "1",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            let actual = example::build_request(&args).unwrap();
            let image = redact(&RedactionInput {
                capture: Bgra::new(2, 1, &pixels).unwrap(),
                region,
                snapshot_region: region,
                fields: &[],
                snapshot_truncated: false,
                unclassified_sensitive_field: false,
                max_edge: 1,
            })
            .unwrap();
            let expected = safety_request(
                model,
                ReasoningEffort::SAFETY_CHECK,
                locale["tray.try_safe"].as_str().unwrap(),
                &render_element_list(&[], 0),
                &image.image,
            );
            assert_eq!(actual, expected);
            let url = actual["input"][0]["content"][1]["image_url"]
                .as_str()
                .unwrap();
            let png = STANDARD
                .decode(url.strip_prefix("data:image/png;base64,").unwrap())
                .unwrap();
            let reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
            assert_eq!((reader.info().width, reader.info().height), (1, 1));
            let mut explicit = args.clone();
            explicit.extend(["--reasoning".to_owned(), "high".to_owned()]);
            assert_eq!(
                example::build_request(&explicit).unwrap()["reasoning"]["effort"],
                "high"
            );
        }
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn example_rejects_bad_arguments_before_reading_image() {
    for args in [
        vec!["--image"],
        vec!["--unknown", "value"],
        vec!["--image", "missing", "--lang", "fr"],
    ] {
        let args: Vec<String> = args.into_iter().map(str::to_owned).collect();
        assert!(example::build_request(&args).is_err());
    }
}
