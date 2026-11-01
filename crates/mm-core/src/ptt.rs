//! Pure push-to-talk transitions (D16/D17). The caller owns the hook, polling,
//! microphone and cues; only Start permits opening the microphone.

use std::collections::{BTreeMap, BTreeSet};

/// A key "held" longer than this before the talk key went down no longer makes a chord:
/// its key-up was most likely lost while Windows dropped a slow hook.
pub const STALE_KEY_MS: u64 = 2_000;

/// The talk key "either Ctrl" (D75): `VK_CONTROL`, which `GetAsyncKeyState` reads as down
/// while either side is. A low-level hook reports the sides, `LEFT_CTRL` and `RIGHT_CTRL`.
pub const EITHER_CTRL: u32 = 0x11;
pub const LEFT_CTRL: u32 = 0xA2;
pub const RIGHT_CTRL: u32 = 0xA3;
/// Left Ctrl starts everyday shortcuts (Ctrl+C, Ctrl+V, Ctrl+Z): either Ctrl must be held
/// alone this long before listening starts.
pub const CTRL_HOLD_MS: u64 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Talk,
    /// One of the keys that together make the talk key (either Ctrl), by virtual key:
    /// the talk key is down while any of them is.
    TalkSide(u32),
    Other(u32),
}

impl Key {
    /// Classify the configured virtual key before chord detection.
    pub fn from_virtual_key(vk: u32, talk_key: u32) -> Self {
        if talk_key == EITHER_CTRL && matches!(vk, EITHER_CTRL | LEFT_CTRL | RIGHT_CTRL) {
            Self::TalkSide(vk)
        } else if vk == talk_key {
            Self::Talk
        } else {
            Self::Other(vk)
        }
    }
}

/// Whether `vk` is the configured talk key or one of its sides.
pub fn is_talk_key(vk: u32, talk_key: u32) -> bool {
    !matches!(Key::from_virtual_key(vk, talk_key), Key::Other(_))
}

/// A lock key used for talking must not change the keyboard's lock state.
pub fn suppress_lock_key(vk: u32, talk_key: u32, injected: bool) -> bool {
    vk == 0x14 && vk == talk_key && !injected
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Hold,
    Toggle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PttSettings {
    pub mode: Mode,
    pub hold_ms: u64,
    pub max_ms: u64,
}

impl Default for PttSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Hold,
            hold_ms: 250,
            max_ms: 60_000,
        }
    }
}

impl PttSettings {
    /// Either Ctrl holds at least `CTRL_HOLD_MS` before listening; other keys keep theirs.
    pub fn for_talk_key(self, talk_key: u32) -> Self {
        if talk_key == EITHER_CTRL {
            Self {
                hold_ms: self.hold_ms.max(CTRL_HOLD_MS),
                ..self
            }
        } else {
            self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PttInput {
    KeyDown {
        key: Key,
        injected: bool,
        now_ms: u64,
    },
    KeyUp {
        key: Key,
        injected: bool,
        now_ms: u64,
    },
    Poll {
        ptt_key_down: bool,
        now_ms: u64,
    },
    Tick {
        now_ms: u64,
    },
    SettingsChanged(PttSettings),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Released,
    Toggled,
    Cancelled,
    Watchdog,
    TooLong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PttEvent {
    Start { generation: u64 },
    Stop { generation: u64, reason: StopReason },
    Cancel { generation: u64 },
}

/// Feed hook events and periodic Tick/Poll inputs in delivery order.
/// Backward timestamps are clamped; injected inputs do not even advance time.
#[derive(Debug, Default)]
pub struct Ptt {
    settings: PttSettings,
    now_ms: u64,
    pressed_at: Option<u64>,
    blocked: bool,
    /// Other keys held now, with when they went down.
    other_keys: BTreeMap<u32, u64>,
    /// The sides of the talk key held now (either Ctrl).
    talk_sides: BTreeSet<u32>,
    started_at: Option<u64>,
    generation: u64,
}

impl Ptt {
    pub fn new(settings: PttSettings) -> Self {
        Self {
            settings,
            ..Self::default()
        }
    }

    /// Whether the push-to-talk key is down as far as the hook said. A lost key-up keeps
    /// it so until a `Poll` clears it: the caller polls while this or a session is on.
    pub fn key_pressed(&self) -> bool {
        self.pressed_at.is_some()
    }

    pub fn input(&mut self, input: PttInput) -> Vec<PttEvent> {
        let input = self.merge_sides(input);
        let mut out = Vec::new();
        match input {
            PttInput::KeyDown { injected: true, .. } | PttInput::KeyUp { injected: true, .. } => {
                return out;
            }
            PttInput::SettingsChanged(settings) => {
                self.stop(StopReason::Cancelled, &mut out);
                self.settings = settings;
                self.blocked = self.pressed_at.is_some();
                return out;
            }
            PttInput::KeyDown { now_ms, .. }
            | PttInput::KeyUp { now_ms, .. }
            | PttInput::Poll { now_ms, .. }
            | PttInput::Tick { now_ms } => self.now_ms = self.now_ms.max(now_ms),
        }

        // Enforce the recording limit on every genuine timed input. Keep a held
        // key blocked afterward so repeats/ticks cannot reopen the microphone.
        if self
            .started_at
            .is_some_and(|start| self.now_ms.saturating_sub(start) >= self.settings.max_ms)
        {
            self.stop(StopReason::TooLong, &mut out);
            self.blocked = self.pressed_at.is_some();
        }

        match input {
            PttInput::KeyDown { key: Key::Talk, .. } => {
                if self.pressed_at.is_none() {
                    self.pressed_at = Some(self.now_ms);
                    // A key pressed just before is a chord (D16); one "held" for long lost
                    // its key-up and must not block PTT for good.
                    let now = self.now_ms;
                    self.other_keys
                        .retain(|_, down| now.saturating_sub(*down) < STALE_KEY_MS);
                    self.blocked = !self.other_keys.is_empty();
                }
            }
            PttInput::KeyDown {
                key: Key::Other(key),
                ..
            } => {
                if self.other_keys.insert(key, self.now_ms).is_none() && self.pressed_at.is_some() {
                    self.blocked = true;
                    if self.started_at.take().is_some() {
                        out.push(PttEvent::Cancel {
                            generation: self.generation,
                        });
                    }
                }
            }
            PttInput::KeyUp { key: Key::Talk, .. } => {
                if let Some(pressed) = self.pressed_at.take() {
                    match self.settings.mode {
                        Mode::Hold => self.stop(StopReason::Released, &mut out),
                        Mode::Toggle
                            if !self.blocked
                                && self.now_ms.saturating_sub(pressed) <= self.settings.hold_ms =>
                        {
                            if self.started_at.is_some() {
                                self.stop(StopReason::Toggled, &mut out);
                            } else {
                                self.start(&mut out);
                            }
                        }
                        Mode::Toggle => {}
                    }
                }
                self.blocked = false;
            }
            PttInput::KeyUp {
                key: Key::Other(key),
                ..
            } => {
                self.other_keys.remove(&key);
            }
            PttInput::Poll {
                ptt_key_down: false,
                ..
            } => {
                if self.settings.mode == Mode::Hold {
                    self.stop(StopReason::Watchdog, &mut out);
                }
                // A poll recovers a lost release, but cannot prove a lone tap.
                self.pressed_at = None;
                self.blocked = false;
            }
            _ => {}
        }

        if self.settings.mode == Mode::Hold
            && !self.blocked
            && self.started_at.is_none()
            && self
                .pressed_at
                .is_some_and(|pressed| self.now_ms.saturating_sub(pressed) >= self.settings.hold_ms)
        {
            self.start(&mut out);
        }
        out
    }

    /// The sides of either Ctrl make one talk key: down with the first side, up with the
    /// last. A side released while another is held only advances time; a release poll
    /// (both up) forgets sides whose key-up was lost.
    fn merge_sides(&mut self, input: PttInput) -> PttInput {
        match input {
            PttInput::KeyDown {
                key: Key::TalkSide(side),
                injected: false,
                now_ms,
            } => {
                self.talk_sides.insert(side);
                PttInput::KeyDown {
                    key: Key::Talk,
                    injected: false,
                    now_ms,
                }
            }
            PttInput::KeyUp {
                key: Key::TalkSide(side),
                injected: false,
                now_ms,
            } => {
                self.talk_sides.remove(&side);
                if self.talk_sides.is_empty() {
                    PttInput::KeyUp {
                        key: Key::Talk,
                        injected: false,
                        now_ms,
                    }
                } else {
                    PttInput::Tick { now_ms }
                }
            }
            PttInput::Poll {
                ptt_key_down: false,
                ..
            } => {
                self.talk_sides.clear();
                input
            }
            _ => input,
        }
    }

    fn start(&mut self, out: &mut Vec<PttEvent>) {
        // Refuse new recordings on exhaustion instead of reusing an identifier.
        if let Some(generation) = self.generation.checked_add(1) {
            self.generation = generation;
            self.started_at = Some(self.now_ms);
            out.push(PttEvent::Start { generation });
        }
    }

    fn stop(&mut self, reason: StopReason, out: &mut Vec<PttEvent>) {
        if self.started_at.take().is_some() {
            out.push(PttEvent::Stop {
                generation: self.generation,
                reason,
            });
        }
    }
}
