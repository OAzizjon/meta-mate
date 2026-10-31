//! The JSON the WebView's timeline player receives (src/player). This snapshot is the
//! reference for the TypeScript types: change both together.

use mm_core::matescript::{Item, Mode, Parser, Settings};

#[test]
fn wire_format() {
    let mut parser = Parser::new(Settings {
        language: "en".into(),
        mode: Mode::Fun,
        point_ids: vec!["e12".into()],
        screenshot: Some((1280, 800)),
        open_request: None,
    });
    let answer = "<lang en><mood happy><point e12><sfx ping soft>It's right here. \
                  <step 1/2 e12><show 125%>Click it.<wait click><pause 400>\
                  <amb rain soft><sfx rimshot solo><point e99>Then this.<amb off><mood proud><end>";
    let mut items: Vec<Item> = parser.push(answer);
    items.extend(parser.finish());
    let json: Vec<String> = items
        .iter()
        .map(|item| serde_json::to_string(item).unwrap())
        .collect();
    insta::assert_snapshot!(json.join("\n"));
}
