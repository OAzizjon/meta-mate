use super::*;

/// The running driver. Dropping it stops the thread within one wake-up and unhooks.
pub struct PttDriver {
    stop: Arc<AtomicBool>,
    silence: Sender<DriverInput>,
    thread: Option<JoinHandle<()>>,
}

impl PttDriver {
    /// A handle the microphone's thread can keep: it ends only its own recording.
    pub fn silencer(&self) -> Silencer {
        Silencer(self.silence.clone())
    }
}

/// Ends one hands-free recording ("press, talk, pause") after the speech ends.
#[derive(Clone)]
pub struct Silencer(Sender<DriverInput>);

impl Silencer {
    /// End only this recording, so a delayed VAD result cannot stop a newer one.
    pub fn silence(&self, generation: u64) {
        let _ = self.0.send(DriverInput::Silence(generation));
    }
}

impl Drop for PttDriver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.silence.send(DriverInput::Wake);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Installs the hook, then drives `Ptt` on a thread of its own; `on_event` runs there,
/// in order.
pub fn spawn(
    settings: PttSettings,
    mut on_event: impl FnMut(PttEvent) + Send + 'static,
) -> Result<PttDriver, HookError> {
    spawn_timed(settings, move |event, _| on_event(event))
}

/// Carries the genuine key-down time, before hold detection or queueing delays.
pub(crate) fn spawn_timed(
    settings: PttSettings,
    mut on_event: impl FnMut(PttEvent, Option<Instant>) + Send + 'static,
) -> Result<PttDriver, HookError> {
    // PttSettings already supplies the startup mode; preserve the spawn API used
    // by voice.rs. Later changes arrive through set_ptt_mode.
    TOGGLE_MODE.store(settings.mode == Mode::Toggle, Ordering::Relaxed);
    let started = Instant::now();
    let (events, keys) = mpsc::channel();
    let hook = keyboard::start_mapped(events.clone(), DriverInput::Key)?;
    let tap = Arc::new(KeyTap::default());
    let published = Arc::downgrade(&tap);
    let stop = Arc::new(AtomicBool::new(false));
    let silence = events.clone();
    let clicks = events.clone();
    let thread = std::thread::Builder::new()
        .name("mm-ptt".into())
        .spawn({
            let stop = stop.clone();
            move || {
                run(
                    Feed::new(settings, started),
                    keys,
                    events,
                    hook,
                    tap,
                    &stop,
                    &mut on_event,
                )
            }
        })
        .map_err(HookError::Thread)?;
    if let Ok(mut current) = KEY_TAP.lock() {
        *current = published;
    }
    if let Ok(mut current) = CLICKS.lock() {
        *current = Some(clicks);
    }
    Ok(PttDriver {
        stop,
        silence,
        thread: Some(thread),
    })
}

fn run(
    mut feed: Feed,
    inputs: Receiver<DriverInput>,
    events: Sender<DriverInput>,
    mut hook: KeyboardHook, // unhooks when the loop ends
    tap: Arc<KeyTap>,
    stop: &AtomicBool,
    on_event: &mut impl FnMut(PttEvent, Option<Instant>),
) {
    #[cfg(debug_assertions)]
    let mut wakes = crate::perf::Wakes::new("ptt");
    let mut emit = |events: Vec<PttEvent>, pressed_at| {
        for event in events {
            on_event(event, pressed_at);
        }
    };
    // The last talk-key event the hook delivered, and the last reinstall.
    let mut last_ctrl = Instant::now();
    let mut reinstalled = Instant::now()
        .checked_sub(REINSTALL_EVERY)
        .unwrap_or_else(Instant::now);
    let mut idle_polled = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let changed = feed.set_key(PTT_KEY.load(Ordering::Relaxed));
        emit(changed, feed.pressed_at);
        match inputs.recv_timeout(feed.wake_interval(Instant::now())) {
            Ok(DriverInput::Key(event)) => {
                tap.send(event);
                if event.vk == feed.vk {
                    last_ctrl = event.at;
                }
                let events = feed.driver_key(event);
                emit(events, feed.pressed_at);
            }
            Ok(DriverInput::Click(at)) => {
                let events = feed.click(at);
                emit(events, feed.pressed_at);
            }
            Ok(DriverInput::Silence(generation)) => {
                let events = feed.silence(generation);
                emit(events, feed.pressed_at);
            }
            Ok(DriverInput::Wake) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        #[cfg(debug_assertions)]
        wakes.wake();
        // Not `events`: that name is the hook's sender, needed for a reinstall below.
        let ticked = feed.tick(now);
        emit(ticked, feed.pressed_at);

        // Windows updates the async key state only after the hook returns: polled right
        // after a talk-key event it can still read the old state. One WAKE later it has
        // caught up; other keys' events do not matter.
        let silent = now.saturating_duration_since(last_ctrl);
        if silent < WAKE {
            continue;
        }
        if !feed.needs_poll() {
            if now.saturating_duration_since(idle_polled) < IDLE_WAKE {
                continue;
            }
            idle_polled = now;
        }
        let down = keyboard::key_down(feed.vk);
        if feed.needs_poll() {
            // An event queued meanwhile goes first; the next wake polls again.
            match inputs.try_recv() {
                Ok(DriverInput::Key(event)) => {
                    tap.send(event);
                    if event.vk == feed.vk {
                        last_ctrl = event.at;
                    }
                    let events = feed.driver_key(event);
                    emit(events, feed.pressed_at);
                }
                Ok(DriverInput::Click(at)) => {
                    let events = feed.click(at);
                    emit(events, feed.pressed_at);
                }
                Ok(DriverInput::Silence(generation)) => {
                    let events = feed.silence(generation);
                    emit(events, feed.pressed_at);
                }
                Ok(DriverInput::Wake) => {}
                Err(_) => {
                    let events = feed.poll(down, now);
                    emit(events, feed.pressed_at);
                }
            }
        } else if down
            && silent >= HOOK_SILENT
            && now.saturating_duration_since(reinstalled) >= REINSTALL_EVERY
        {
            // Held, yet the hook never said so: Windows removed it.
            reinstalled = now;
            if let Ok(fresh) = keyboard::start_mapped(events.clone(), DriverInput::Key) {
                hook = fresh;
            }
        }
    }
    drop(hook);
}

/// `--ptt-probe`: no window; prints every `PttEvent` with the milliseconds since the
/// start for 60 s, then exits 0 (1 when the hook cannot be installed). None without
/// the flag.
pub fn probe_if_requested(args: &[String]) -> Option<i32> {
    if !args.iter().any(|arg| arg == "--ptt-probe") {
        return None;
    }
    println!("PTT probe: hold the configured talk key in any app; ends in 60 s");
    let started = Instant::now();
    let driver = spawn(PttSettings::default(), move |event| {
        println!("{:>6} ms  {event:?}", started.elapsed().as_millis());
    });
    let driver = match driver {
        Ok(driver) => driver,
        Err(error) => {
            eprintln!("PTT probe: {error}");
            return Some(1);
        }
    };
    std::thread::sleep(PROBE);
    drop(driver);
    Some(0)
}
