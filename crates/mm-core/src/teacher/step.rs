use crate::api::response::Action;
use crate::geom::PhysRect;

pub(super) struct Step {
    pub action: Action,
    pub rect: PhysRect,
    pub call: usize,
}

pub(super) fn immediate(action: &Action) -> bool {
    matches!(
        action,
        Action::Wait | Action::Screenshot | Action::Move { .. }
    )
}

pub(super) fn supported(action: &Action) -> bool {
    match action {
        Action::Click { button, .. } => button == "left",
        Action::Scroll { .. } | Action::Drag { .. } => false,
        _ => true,
    }
}

pub(super) fn same_chord(expected: &[String], actual: &[String]) -> bool {
    let normalize = |keys: &[String]| {
        let mut keys: Vec<_> = keys.iter().map(|key| key.to_ascii_lowercase()).collect();
        keys.sort();
        keys.dedup();
        keys
    };
    !expected.is_empty() && normalize(expected) == normalize(actual)
}
