//! Pointing benchmark request and first-tag parsing use the app's own primitives.
#[allow(dead_code)]
#[path = "../examples/point_parse.rs"]
mod parse;
#[allow(dead_code)]
#[path = "../examples/point_request.rs"]
mod request;

use mm_core::matescript::Point;
use mm_core::shot::encode_png_rgb;
use std::fs;

#[test]
fn first_point_preserves_source_order_and_parser_recovery() {
    for (answer, expected) in [
        ("<point e1><point e2>Here.", Some(Point::Id("e1".into()))),
        ("<point 20,30>Here.", Some(Point::Pixel(20, 30))),
        ("<point e999>", Some(Point::Id("e999".into()))),
        ("<point  1,2><point 3,4>", Some(Point::Pixel(3, 4))),
        ("<end><point e1>", None),
        ("<point -1,0>", None),
        ("<point e1", None),
        ("I can't find it.", None),
    ] {
        assert_eq!(
            parse::first_point(answer).map(|(p, _)| p),
            expected,
            "{answer}"
        );
    }
    let answer = "Вот: <point e1>";
    assert_eq!(parse::first_point(answer).unwrap().1, answer.len());
}

#[test]
fn request_maps_elements_once_for_the_encoded_frame() {
    let folder = std::env::temp_dir().join(format!("mm-point-example-{}", std::process::id()));
    fs::create_dir_all(&folder).unwrap();
    let image = folder.join("frame.png");
    let elements = folder.join("elements.json");
    fs::write(
        &image,
        encode_png_rgb(&vec![255; 1280 * 800 * 3], 1280, 800).unwrap(),
    )
    .unwrap();
    fs::write(
        &elements,
        r#"[{"id":"e1","role":"button","name":"Send","rect":[100,80,100,40]}]"#,
    )
    .unwrap();
    for model in ["muse-spark-1.3", "muse-wonder-1.0"] {
        for (max_edge, expected) in [("1280", "[100,80,100,40]"), ("640", "[50,40,50,20]")] {
            for mode in [elements.to_str().unwrap(), "none"] {
                let args: Vec<String> = [
                    "--image",
                    image.to_str().unwrap(),
                    "--question",
                    "Where is Send?",
                    "--model",
                    model,
                    "--max-edge",
                    max_edge,
                    "--elements",
                    mode,
                ]
                .into_iter()
                .map(str::to_owned)
                .collect();
                let body = request::build_request(&args).unwrap();
                assert_eq!(body["model"], model);
                assert_eq!(body["reasoning"]["effort"], "minimal");
                assert!(body.get("tools").is_none());
                assert_eq!(body["stream"], true);
                let text = body["input"][0]["content"][0]["text"].as_str().unwrap();
                assert!(text.contains("Where is Send?"));
                if mode == "none" {
                    assert!(!text.contains("e1 button"));
                } else {
                    assert!(text.contains(&format!("e1 button \"Send\" {expected}")));
                }
                let mut explicit = args.clone();
                explicit.extend(["--reasoning".into(), "high".into()]);
                assert_eq!(
                    request::build_request(&explicit).unwrap()["reasoning"]["effort"],
                    "high"
                );
            }
        }
    }
    fs::remove_file(image).unwrap();
    fs::remove_file(elements).unwrap();
    fs::remove_dir(folder).unwrap();
}

#[test]
fn request_rejects_bad_flags_and_models() {
    for args in [
        vec!["--image"],
        vec!["--unknown", "value"],
        vec!["--model", "other"],
    ] {
        let args: Vec<String> = args.into_iter().map(str::to_owned).collect();
        assert!(request::build_request(&args).is_err());
    }
}
