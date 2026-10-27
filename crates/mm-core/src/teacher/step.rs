use crate::api::response::Action;
use crate::geom::{PhysPoint, PhysRect};

use super::canonical_key_name;

pub(super) struct Step {
    pub action: Action,
    pub rect: PhysRect,
    pub drag_end: Option<PhysPoint>,
    pub call: usize,
}

pub(super) fn immediate(action: &Action) -> bool {
    matches!(
        action,
        Action::Wait | Action::Screenshot | Action::Move { .. }
    )
}

pub(super) fn supported(action: &Action, keyboard_steps: bool) -> bool {
    match action {
        Action::Click { button, .. } => button == "left",
        Action::Scroll { .. } => false,
        Action::Type { .. } | Action::Keypress { .. } => keyboard_steps,
        _ => true,
    }
}

pub(super) fn same_chord(expected: &[String], actual: &[String]) -> bool {
    let normalize = |keys: &[String]| {
        let mut keys: Vec<_> = keys
            .iter()
            .map(|key| canonical_key_name(&key.to_ascii_lowercase()).to_owned())
            .collect();
        keys.sort();
        keys.dedup();
        keys
    };
    !expected.is_empty() && normalize(expected) == normalize(actual)
}

#[cfg(test)]
mod tests {
    use super::same_chord;

    #[test]
    fn aliases_are_folded_before_sorting_and_deduplication() {
        let expected = ["CONTROL", "ctrl", "Return"].map(String::from);
        let actual = ["enter", "rctrl"].map(String::from);
        assert!(same_chord(&expected, &actual));
        assert!(!same_chord(&expected, &["enter".into()]));
        assert!(!same_chord(&[], &[]));
    }
}
