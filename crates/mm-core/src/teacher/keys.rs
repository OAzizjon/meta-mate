use std::collections::BTreeMap;

use crate::ptt::STALE_KEY_MS;

use super::TeacherInput;

/// The foreground window at key-down, before the app can react to the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyWindow {
    pub handle: u64,
    pub process_id: u32,
}

/// Model key names from virtual-key codes, independent of the keyboard layout.
pub fn vk_to_key_name(vk: u32) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r",
        "s", "t", "u", "v", "w", "x", "y", "z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const FUNCTION: [&str; 12] = [
        "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
    ];
    Some(match vk {
        0x11 | 0xA2 | 0xA3 => "ctrl",
        0x10 | 0xA0 | 0xA1 => "shift",
        0x12 | 0xA4 | 0xA5 => "alt",
        0x5B | 0x5C => "win",
        0x41..=0x5A => LETTERS[(vk - 0x41) as usize],
        0x30..=0x39 => DIGITS[(vk - 0x30) as usize],
        0x60..=0x69 => DIGITS[(vk - 0x60) as usize],
        0x70..=0x7B => FUNCTION[(vk - 0x70) as usize],
        0x0D => "enter",
        0x1B => "esc",
        0x09 => "tab",
        0x20 => "space",
        0x08 => "backspace",
        0x2E => "delete",
        0x2D => "insert",
        0x24 => "home",
        0x23 => "end",
        0x21 => "pageup",
        0x22 => "pagedown",
        0x25 => "left",
        0x26 => "up",
        0x27 => "right",
        0x28 => "down",
        0xBB | 0x6B => "plus",
        0xBD | 0x6D => "minus",
        0xBC => "comma",
        0xBE | 0x6E => "period",
        0xBF | 0x6F => "slash",
        _ => return None,
    })
}

/// Normalize aliases after the caller folds model strings to lowercase.
pub fn canonical_key_name(raw: &str) -> &str {
    match raw {
        "control" | "ctrl" | "lctrl" | "rctrl" => "ctrl",
        "return" | "enter" => "enter",
        "escape" | "esc" => "esc",
        " " | "spacebar" | "space" => "space",
        "+" | "equal" | "equals" | "add" | "plus" => "plus",
        "-" | "subtract" | "hyphen" | "minus" => "minus",
        "del" | "delete" => "delete",
        "ins" | "insert" => "insert",
        "pgup" | "page_up" | "pageup" => "pageup",
        "pgdn" | "page_down" | "pagedown" => "pagedown",
        "arrowup" => "up",
        "arrowdown" => "down",
        "arrowleft" => "left",
        "arrowright" => "right",
        "command" | "meta" | "super" | "windows" | "win" => "win",
        _ => raw,
    }
}

fn is_modifier(vk: u32) -> bool {
    matches!(vk, 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
}

/// Converts key edges into chords without completing any teacher step itself.
#[derive(Default)]
pub struct ChordRecognizer {
    now_ms: u64,
    modifiers: BTreeMap<&'static str, (u64, bool)>,
    pending_solo_modifier: Option<(&'static str, bool, Option<KeyWindow>)>,
}

impl ChordRecognizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed edges in delivery order; backward timestamps are clamped.
    pub fn key(
        &mut self,
        vk: u32,
        down: bool,
        injected: bool,
        now_ms: u64,
    ) -> Option<TeacherInput> {
        self.key_in_window(vk, down, injected, now_ms, None)
    }

    pub fn key_in_window(
        &mut self,
        vk: u32,
        down: bool,
        injected: bool,
        now_ms: u64,
        foreground: Option<KeyWindow>,
    ) -> Option<TeacherInput> {
        self.now_ms = self.now_ms.max(now_ms);
        self.modifiers
            .retain(|_, (at, _)| self.now_ms.saturating_sub(*at) <= STALE_KEY_MS);
        if let Some((name, _, _)) = self.pending_solo_modifier
            && !self.modifiers.contains_key(name)
        {
            self.pending_solo_modifier = None;
        }
        let name = vk_to_key_name(vk);
        if is_modifier(vk) {
            let name = name?;
            if down {
                if let Some((_, was_injected)) = self.modifiers.get_mut(name) {
                    // Repeats preserve the original timestamp and software provenance.
                    *was_injected |= injected;
                    if let Some((_, pending_injected, _)) = &mut self.pending_solo_modifier {
                        *pending_injected |= injected;
                    }
                } else {
                    self.pending_solo_modifier = if self.modifiers.is_empty() {
                        Some((name, injected, foreground))
                    } else {
                        None
                    };
                    self.modifiers.insert(name, (self.now_ms, injected));
                }
            } else {
                let (_, was_injected) = self.modifiers.remove(name)?;
                if let Some((pending, pending_injected, foreground)) = self.pending_solo_modifier
                    && pending == name
                {
                    self.pending_solo_modifier = None;
                    return Some(TeacherInput::KeyChord {
                        foreground,
                        keys: vec![name.into()],
                        injected: injected || was_injected || pending_injected,
                        now_ms: self.now_ms,
                    });
                }
            }
            return None;
        }
        if !down {
            return None;
        }
        // Even an unmapped key cancels a solo modifier tap.
        self.pending_solo_modifier = None;
        let name = name?;
        let mut keys: Vec<String> = self.modifiers.keys().map(|key| (*key).into()).collect();
        keys.push(name.into());
        Some(TeacherInput::KeyChord {
            foreground,
            keys,
            injected: injected || self.modifiers.values().any(|(_, injected)| *injected),
            now_ms: self.now_ms,
        })
    }
}
