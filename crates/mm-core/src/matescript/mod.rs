//! MateScript: the streaming markup the model writes (docs/matescript.md).

mod parser;
mod sentence;
mod settings;
mod tags;
mod timeline;
mod vocabulary;

pub use parser::Parser;
pub use settings::{Mode, Settings};
pub use timeline::{Bound, Item, Level, Point, Say, Sound, Step};
