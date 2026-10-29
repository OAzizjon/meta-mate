use super::*;

/// Hook events and wake-ups as `PttInput`s on one clock, and whether a session is on
/// (between Start and Stop or Cancel). Pure: the caller passes every `Instant`.
pub(super) struct Feed {
    click_active: bool,
    suppress_key: bool,
    pub(super) vk: u32,
    pub(super) pressed_at: Option<Instant>,
    ptt: Ptt,
    settings: PttSettings,
    started: Instant,
    active: bool,
    generation: u64,
    active_since: u64,
    now_ms: u64,
    /// The core validates chords, injected events and repeats. In toggle mode
    /// its zero-delay hold is a gesture, not permission to open the microphone.
    gesture: Option<u64>,
}

impl Feed {
    pub(super) fn new(settings: PttSettings, started: Instant) -> Self {
        Self {
            click_active: false,
            suppress_key: false,
            vk: RIGHT_CTRL,
            pressed_at: None,
            ptt: Ptt::new(Self::core_settings(settings)),
            settings,
            started,
            active: false,
            generation: 0,
            active_since: 0,
            now_ms: 0,
            gesture: None,
        }
    }

    fn core_settings(mut settings: PttSettings) -> PttSettings {
        if settings.mode == Mode::Toggle {
            settings.mode = Mode::Hold;
            settings.hold_ms = 0;
            settings.max_ms = u64::MAX;
        }
        settings
    }

    pub(super) fn set_key(&mut self, vk: u32) -> Vec<PttEvent> {
        if self.vk == vk {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.end(StopReason::Cancelled, &mut out);
        self.vk = vk;
        self.ptt = Ptt::new(Self::core_settings(self.settings));
        self.gesture = None;
        self.pressed_at = None;
        out
    }

    pub(super) fn driver_key(&mut self, event: KeyEvent) -> Vec<PttEvent> {
        let mode = if TOGGLE_MODE.load(Ordering::Relaxed) {
            Mode::Toggle
        } else {
            Mode::Hold
        };
        self.key_in_mode(event, mode)
    }

    pub(super) fn key_in_mode(&mut self, event: KeyEvent, mode: Mode) -> Vec<PttEvent> {
        let mut out = Vec::new();
        if event.vk == self.vk
            && event.down
            && !event.injected
            && !self.click_active
            && !self.suppress_key
            && !self.ptt.key_pressed()
            && self.settings.mode != mode
        {
            self.end(StopReason::Cancelled, &mut out);
            self.settings.mode = mode;
            self.gesture = None;
            let _ = self
                .ptt
                .input(PttInput::SettingsChanged(Self::core_settings(
                    self.settings,
                )));
        }
        out.extend(self.key(event));
        out
    }

    pub(super) fn silence(&mut self, generation: u64) -> Vec<PttEvent> {
        let mut out = Vec::new();
        if self.active
            && (self.click_active || self.settings.mode == Mode::Toggle)
            && generation == self.generation
        {
            self.end(StopReason::Toggled, &mut out);
            self.gesture = None;
        }
        out
    }

    #[cfg(test)]
    pub(super) fn active(&self) -> bool {
        self.active
    }

    /// Polling is due during a session and while a press the hook reported is pending
    /// (after TooLong or a chord too): a lost key-up must never wedge the next press.
    pub(super) fn needs_poll(&self) -> bool {
        self.active || self.ptt.key_pressed() || self.suppress_key
    }

    pub(super) fn key(&mut self, event: KeyEvent) -> Vec<PttEvent> {
        if event.vk == self.vk && !event.injected {
            if self.click_active && event.down {
                let mut out = Vec::new();
                self.end(StopReason::Toggled, &mut out);
                self.suppress_key = true;
                return out;
            }
            if self.suppress_key {
                if !event.down {
                    self.suppress_key = false;
                }
                return Vec::new();
            }
        }
        if event.vk == self.vk && event.down && !event.injected && !self.ptt.key_pressed() {
            self.pressed_at = Some(event.at);
        }
        let key = Key::from_virtual_key(event.vk, self.vk);
        let (injected, now_ms) = (event.injected, self.ms(event.at));
        self.input(if event.down {
            PttInput::KeyDown {
                key,
                injected,
                now_ms,
            }
        } else {
            PttInput::KeyUp {
                key,
                injected,
                now_ms,
            }
        })
    }

    pub(super) fn tick(&mut self, now: Instant) -> Vec<PttEvent> {
        let now_ms = self.ms(now);
        self.input(PttInput::Tick { now_ms })
    }

    pub(super) fn poll(&mut self, ptt_key_down: bool, now: Instant) -> Vec<PttEvent> {
        if !ptt_key_down {
            self.suppress_key = false;
        }
        let now_ms = self.ms(now);
        self.input(PttInput::Poll {
            ptt_key_down,
            now_ms,
        })
    }

    fn input(&mut self, input: PttInput) -> Vec<PttEvent> {
        let time = match input {
            PttInput::KeyDown { injected: true, .. } | PttInput::KeyUp { injected: true, .. } => {
                return Vec::new();
            }
            PttInput::KeyDown { now_ms, .. }
            | PttInput::KeyUp { now_ms, .. }
            | PttInput::Tick { now_ms }
            | PttInput::Poll { now_ms, .. } => now_ms,
            PttInput::SettingsChanged(_) => self.now_ms,
        };
        self.now_ms = self.now_ms.max(time);
        if self.click_active {
            let mut out = Vec::new();
            if self.now_ms.saturating_sub(self.active_since)
                >= TOGGLE_MAX_MS.min(self.settings.max_ms)
            {
                self.end(StopReason::TooLong, &mut out);
            }
            return out;
        }
        let events = self.ptt.input(input);
        let mut out = Vec::new();
        if self.settings.mode == Mode::Hold {
            for event in events {
                match event {
                    PttEvent::Start { .. } => self.begin(&mut out),
                    PttEvent::Stop { reason, .. } => self.end(reason, &mut out),
                    PttEvent::Cancel { .. } => self.cancel(&mut out),
                }
            }
            return out;
        }
        // The logical recording has its own deadline, including time with the
        // key released. Expiration/silence also suppress the pending gesture.
        if self.active
            && self.now_ms.saturating_sub(self.active_since)
                >= TOGGLE_MAX_MS.min(self.settings.max_ms)
        {
            self.end(StopReason::TooLong, &mut out);
            self.gesture = None;
        }
        for event in events {
            match event {
                PttEvent::Start { .. } => self.gesture = Some(self.now_ms),
                PttEvent::Cancel { .. } => {
                    self.gesture = None;
                    self.cancel(&mut out);
                }
                PttEvent::Stop { reason, .. } => {
                    if let Some(pressed) = self.gesture.take() {
                        let short = self.now_ms.saturating_sub(pressed) < TAP_MS;
                        if self.active && (reason != StopReason::Watchdog || !short) {
                            let reason = if short { StopReason::Toggled } else { reason };
                            self.end(reason, &mut out);
                        } else if reason == StopReason::Released && short {
                            self.begin(&mut out);
                        }
                    }
                }
            }
        }
        if self
            .gesture
            .is_some_and(|pressed| self.now_ms.saturating_sub(pressed) >= TAP_MS)
            && !self.active
        {
            self.begin(&mut out);
        }
        out
    }

    fn begin(&mut self, out: &mut Vec<PttEvent>) {
        if let Some(generation) = self.generation.checked_add(1) {
            self.generation = generation;
            self.active = true;
            self.active_since = self.now_ms;
            out.push(PttEvent::Start { generation });
        }
    }

    pub(super) fn click(&mut self, at: Instant) -> Vec<PttEvent> {
        self.now_ms = self.now_ms.max(self.ms(at));
        let mut out = Vec::new();
        let was_active = self.active;
        let key_pending = self.ptt.key_pressed() || self.suppress_key;
        self.end(StopReason::Toggled, &mut out);
        self.gesture = None;
        // Reset a pending hold too; its release/repeats must not reopen the mic.
        self.ptt = Ptt::new(Self::core_settings(self.settings));
        self.suppress_key = key_pending;
        if !was_active && !key_pending {
            self.pressed_at = None;
            self.begin(&mut out);
            self.click_active = self.active;
        }
        out
    }

    fn end(&mut self, reason: StopReason, out: &mut Vec<PttEvent>) {
        self.click_active = false;
        if self.active {
            self.active = false;
            out.push(PttEvent::Stop {
                generation: self.generation,
                reason,
            });
        }
    }

    fn cancel(&mut self, out: &mut Vec<PttEvent>) {
        self.click_active = false;
        if self.active {
            self.active = false;
            out.push(PttEvent::Cancel {
                generation: self.generation,
            });
        }
    }

    /// Milliseconds since the driver started; an earlier `Instant` counts as 0.
    pub(super) fn ms(&self, at: Instant) -> u64 {
        u64::try_from(at.saturating_duration_since(self.started).as_millis()).unwrap_or(u64::MAX)
    }
}
