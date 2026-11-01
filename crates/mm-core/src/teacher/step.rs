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

fn normalize_chord(keys: &[String]) -> Vec<String> {
    let mut keys: Vec<_> = keys
        .iter()
        .map(|key| canonical_key_name(&key.to_ascii_lowercase()).to_owned())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

fn is_modifier_name(key: &str) -> bool {
    matches!(key, "ctrl" | "shift" | "alt" | "win")
}

pub(super) fn same_chord(expected: &[String], actual: &[String]) -> bool {
    !expected.is_empty() && normalize_chord(expected) == normalize_chord(actual)
}

pub(super) fn wrong_chord(expected: &[String], actual: &[String]) -> bool {
    if expected.is_empty() {
        return false;
    }
    let expected = normalize_chord(expected);
    let actual = normalize_chord(actual);
    if expected == actual {
        return false;
    }
    actual
        .iter()
        .any(|key| is_modifier_name(key) && expected.contains(key))
        || (actual.len() == 1 && !actual[0].is_empty() && !is_modifier_name(&actual[0]))
}

pub(super) fn step_has_ctrl(action: &Action) -> bool {
    matches!(action, Action::Keypress { keys } if keys.iter().any(|k| {
        canonical_key_name(&k.to_ascii_lowercase()) == "ctrl"
    }))
}

#[cfg(test)]
mod tests {
    use super::{same_chord, step_has_ctrl, wrong_chord};
    use crate::api::response::Action;

    #[test]
    fn aliases_are_folded_before_sorting_and_deduplication() {
        let expected = ["CONTROL", "ctrl", "Return"].map(String::from);
        let actual = ["enter", "rctrl"].map(String::from);
        assert!(same_chord(&expected, &actual));
        assert!(!same_chord(&expected, &["enter".into()]));
        assert!(!same_chord(&[], &[]));
    }

    #[test]
    fn wrong_chord_matches_shared_modifiers_and_single_non_modifier_keys() {
        let ctrl_s = ["CTRL", "S"].map(String::from);
        assert!(!wrong_chord(&ctrl_s, &["s".into(), "rctrl".into()]));
        assert!(wrong_chord(&ctrl_s, &["lctrl".into(), "a".into()]));
        assert!(wrong_chord(&ctrl_s, &["rctrl".into()]));
        assert!(wrong_chord(&ctrl_s, &["s".into()]));
        assert!(!wrong_chord(&ctrl_s, &["alt".into()]));
        assert!(!wrong_chord(&ctrl_s, &["shift".into(), "s".into()]));
        assert!(!wrong_chord(&[], &["s".into()]));
    }

    #[test]
    fn step_has_ctrl_recognizes_ctrl_chord_variants() {
        assert!(step_has_ctrl(&Action::Keypress {
            keys: vec!["Ctrl".into(), "S".into()]
        }));
        assert!(step_has_ctrl(&Action::Keypress {
            keys: vec!["Control".into(), "Shift".into(), "Esc".into()]
        }));
        assert!(step_has_ctrl(&Action::Keypress {
            keys: vec!["rctrl".into(), "c".into()]
        }));
        assert!(!step_has_ctrl(&Action::Keypress {
            keys: vec!["Alt".into(), "Tab".into()]
        }));
        assert!(!step_has_ctrl(&Action::Click {
            x: 0,
            y: 0,
            button: "left".into(),
        }));
    }
}
