use mm_core::teacher::{ChordRecognizer, TeacherInput, canonical_key_name, vk_to_key_name};

fn assert_chord(input: Option<TeacherInput>, expected: &[&str], injected: bool, now_ms: u64) {
    match input {
        Some(TeacherInput::KeyChord {
            foreground: _,
            mut keys,
            injected: actual_injected,
            now_ms: actual_ms,
        }) => {
            keys.sort();
            let mut expected = expected.to_vec();
            expected.sort();
            assert_eq!(keys, expected);
            assert_eq!((actual_injected, actual_ms), (injected, now_ms));
        }
        other => panic!("expected KeyChord, got {other:?}"),
    }
}

#[test]
fn modifier_mappings_include_generic_left_and_right() {
    for (codes, name) in [
        (&[0x11, 0xA2, 0xA3][..], "ctrl"),
        (&[0x10, 0xA0, 0xA1][..], "shift"),
        (&[0x12, 0xA4, 0xA5][..], "alt"),
        (&[0x5B, 0x5C][..], "win"),
    ] {
        for &vk in codes {
            assert_eq!(vk_to_key_name(vk), Some(name));
        }
    }
}

#[test]
fn letters_digits_numpad_and_function_keys() {
    for (index, letter) in ('a'..='z').enumerate() {
        assert_eq!(
            vk_to_key_name(0x41 + index as u32),
            Some(letter.to_string().as_str())
        );
    }
    for digit in 0..10 {
        for base in [0x30, 0x60] {
            assert_eq!(
                vk_to_key_name(base + digit),
                Some(digit.to_string().as_str())
            );
        }
    }
    for number in 1..=12 {
        assert_eq!(
            vk_to_key_name(0x6F + number),
            Some(format!("f{number}").as_str())
        );
    }
}

#[test]
fn navigation_editing_and_symbols() {
    for (vk, name) in [
        (0x0D, "enter"),
        (0x1B, "esc"),
        (0x09, "tab"),
        (0x20, "space"),
        (0x08, "backspace"),
        (0x2E, "delete"),
        (0x2D, "insert"),
        (0x24, "home"),
        (0x23, "end"),
        (0x21, "pageup"),
        (0x22, "pagedown"),
        (0x25, "left"),
        (0x26, "up"),
        (0x27, "right"),
        (0x28, "down"),
        (0xBB, "plus"),
        (0x6B, "plus"),
        (0xBD, "minus"),
        (0x6D, "minus"),
        (0xBC, "comma"),
        (0xBE, "period"),
        (0x6E, "period"),
        (0xBF, "slash"),
        (0x6F, "slash"),
    ] {
        assert_eq!(vk_to_key_name(vk), Some(name));
    }
    for vk in [0, 0x07, 0x7C, 0xFF, u32::MAX] {
        assert_eq!(vk_to_key_name(vk), None);
    }
}

#[test]
fn all_model_aliases_and_unknown_names() {
    for (aliases, name) in [
        (&["control", "ctrl", "lctrl", "rctrl"][..], "ctrl"),
        (&["return", "enter"][..], "enter"),
        (&["escape", "esc"][..], "esc"),
        (&[" ", "spacebar", "space"][..], "space"),
        (&["+", "equal", "equals", "add", "plus"][..], "plus"),
        (&["-", "subtract", "hyphen", "minus"][..], "minus"),
        (&["del", "delete"][..], "delete"),
        (&["ins", "insert"][..], "insert"),
        (&["pgup", "page_up", "pageup"][..], "pageup"),
        (&["pgdn", "page_down", "pagedown"][..], "pagedown"),
        (&["arrowup"][..], "up"),
        (&["arrowdown"][..], "down"),
        (&["arrowleft"][..], "left"),
        (&["arrowright"][..], "right"),
        (&["command", "meta", "super", "windows", "win"][..], "win"),
    ] {
        for alias in aliases {
            assert_eq!(canonical_key_name(alias), name);
        }
    }
    for name in ["a", "f12", "shift", "alt", "unknown", ""] {
        assert_eq!(canonical_key_name(name), name);
    }
}

#[test]
fn chords_complete_on_non_modifier_down_and_keep_held_modifiers() {
    let mut keys = ChordRecognizer::new();
    for vk in [0xA2, 0xA1, 0xA4, 0x5C] {
        assert!(keys.key(vk, true, false, 10).is_none());
    }
    assert_chord(
        keys.key(0x41, true, false, 20),
        &["ctrl", "shift", "alt", "win", "a"],
        false,
        20,
    );
    assert!(keys.key(0x41, false, false, 21).is_none());
    assert_chord(
        keys.key(0xBB, true, false, 22),
        &["ctrl", "shift", "alt", "win", "plus"],
        false,
        22,
    );
    for vk in [0xA2, 0xA1, 0xA4, 0x5C] {
        assert!(keys.key(vk, false, false, 30).is_none());
    }
    assert_chord(keys.key(0x74, true, false, 31), &["f5"], false, 31);
}

#[test]
fn solo_modifier_tap_completes_only_on_release() {
    for vk in [0x11, 0xA3, 0x10, 0xA0, 0x12, 0xA5, 0x5B, 0x5C] {
        let mut keys = ChordRecognizer::new();
        assert!(keys.key(vk, false, false, 0).is_none());
        assert!(keys.key(vk, true, false, 10).is_none());
        assert_chord(
            keys.key(vk, false, false, 20),
            &[vk_to_key_name(vk).unwrap()],
            false,
            20,
        );
        assert!(keys.key(vk, false, false, 21).is_none());
    }
}

#[test]
fn other_modifier_or_unmapped_key_cancels_solo_tap() {
    for other in [0x10, 0xFF] {
        let mut keys = ChordRecognizer::new();
        keys.key(0x11, true, false, 0);
        assert!(keys.key(other, true, false, 1).is_none());
        assert!(keys.key(other, false, false, 2).is_none());
        assert!(keys.key(0x11, false, false, 3).is_none());
    }
}

#[test]
fn modifiers_expire_after_2000_ms_including_solo_taps() {
    for (age, held) in [(2000, true), (2001, false)] {
        let mut keys = ChordRecognizer::new();
        keys.key(0x11, true, true, 10);
        let expected = if held {
            vec!["ctrl", "enter"]
        } else {
            vec!["enter"]
        };
        assert_chord(
            keys.key(0x0D, true, false, 10 + age),
            &expected,
            held,
            10 + age,
        );
        let mut keys = ChordRecognizer::new();
        keys.key(0x11, true, false, 10);
        let tap = keys.key(0x11, false, false, 10 + age);
        if held {
            assert_chord(tap, &["ctrl"], false, 10 + age);
        } else {
            assert!(tap.is_none());
        }
    }
}

#[test]
fn injected_provenance_from_modifier_key_and_solo_release() {
    for (modifier, key) in [(true, false), (false, true), (true, true)] {
        let mut keys = ChordRecognizer::new();
        keys.key(0x11, true, modifier, 0);
        assert_chord(keys.key(0x41, true, key, 1), &["ctrl", "a"], true, 1);
    }
    for (down, up) in [(true, false), (false, true)] {
        let mut keys = ChordRecognizer::new();
        keys.key(0x5B, true, down, 0);
        assert_chord(keys.key(0x5B, false, up, 1), &["win"], true, 1);
    }
}

#[test]
fn repeats_preserve_age_and_injection_and_backward_time_is_clamped() {
    let mut keys = ChordRecognizer::new();
    keys.key(0x11, true, false, 100);
    keys.key(0x11, true, true, 90);
    assert_chord(keys.key(0x11, false, false, 80), &["ctrl"], true, 100);
    keys.key(0x11, true, false, 200);
    keys.key(0x11, true, false, 2000);
    assert_chord(keys.key(0x41, true, false, 2201), &["a"], false, 2201);
}

#[test]
fn chords_keep_the_key_down_window() {
    use mm_core::teacher::KeyWindow;
    let app = Some(KeyWindow {
        handle: 12,
        process_id: 34,
    });
    let other = Some(KeyWindow {
        handle: 56,
        process_id: 78,
    });
    let mut keys = ChordRecognizer::new();
    keys.key_in_window(0xA2, true, false, 1, app);
    // A solo modifier is emitted on release, after the app may have moved focus.
    let tap = keys.key_in_window(0xA2, false, false, 2, other);
    assert!(matches!(tap, Some(TeacherInput::KeyChord { foreground, .. }) if foreground == app));
    keys.key_in_window(0xA2, true, false, 3, other);
    // For a chord, the action key owns the window, rather than the modifier.
    let chord = keys.key_in_window(0x4E, true, false, 4, app);
    assert!(matches!(chord, Some(TeacherInput::KeyChord { foreground, .. }) if foreground == app));
    let unknown = keys.key_in_window(0x1B, true, false, 5, None);
    assert!(matches!(
        unknown,
        Some(TeacherInput::KeyChord {
            foreground: None,
            ..
        })
    ));
}
