//! Benchmark first-point extraction, using the app's MateScript grammar.
use mm_core::matescript::{Bound, Item, Parser, Point, Settings};
use std::error::Error;
use std::io::{self, Read};

fn main() -> Result<(), Box<dyn Error>> {
    let mut answer = String::new();
    io::stdin().read_to_string(&mut answer)?;
    println!("{}", serde_json::to_string(&first_point(&answer))?);
    Ok(())
}

pub(crate) fn first_point(answer: &str) -> Option<(Point, usize)> {
    // Admit unknown ids/coordinates here so an invented target scores WRONG rather
    // than a misleading NO-POINT. The scorer checks target ids and frame bounds.
    let ids = answer
        .split("<point ")
        .skip(1)
        .filter_map(|s| s.split_once('>').map(|(id, _)| id))
        .filter(|id| {
            id.strip_prefix('e')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(str::to_owned)
        .collect();
    let settings = Settings {
        point_ids: ids,
        screenshot: Some((u32::MAX, u32::MAX)),
        ..Settings::default()
    };
    // Flushing each completed prefix preserves the first tag even when a later
    // point would replace the sentence's pending point in the playback timeline.
    for (end, ch) in answer.char_indices() {
        if ch != '>' {
            continue;
        }
        let mut parser = Parser::new(settings.clone());
        let mut items = parser.push(&answer[..end + 1]);
        items.extend(parser.finish());
        for item in items {
            let bound = match item {
                Item::Say(say) => say.bound,
                Item::Bound(bound) => vec![bound],
                _ => Vec::new(),
            };
            for action in bound {
                if let Bound::Point(point) = action {
                    return Some((point, end + 1));
                }
            }
        }
    }
    None
}
