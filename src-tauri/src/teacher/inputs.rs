use super::*;

/// The person's clicks while a lesson runs: the low-level mouse hook, recognized into
/// clicks and double clicks, sent to the lesson. Dropping it removes the hook.
pub(super) struct Clicks {
    _hook: mm_win32::mouse::MouseHook,
}

pub(super) fn listen_for_clicks(
    started: Instant,
    inputs: mpsc::UnboundedSender<TeacherInput>,
) -> Option<Clicks> {
    let (events, received) = std::sync::mpsc::channel();
    let hook = mm_win32::mouse::start(events)
        .map_err(|error| eprintln!("teacher: mouse hook: {error}"))
        .ok()?;
    let (double_ms, cx, cy) = mm_win32::mouse::double_click_rules();
    std::thread::Builder::new()
        .name("mm-clicks".into())
        .spawn(move || {
            let mut recognizer = ClickRecognizer::new(double_ms, cx, cy);
            // Ends when the hook is dropped (its sender goes) or the lesson is gone.
            while let Ok(event) = received.recv() {
                let now_ms =
                    u64::try_from(event.time.saturating_duration_since(started).as_millis())
                        .unwrap_or(u64::MAX);
                let left = matches!(event.button, mm_win32::mouse::Button::Left);
                for input in recognizer.button(left, event.down, event.at, event.injected, now_ms) {
                    if inputs.send(input).is_err() {
                        return;
                    }
                }
            }
        })
        .ok()?;
    Some(Clicks { _hook: hook })
}

/// The lesson owns the subscription, so aborting its future disconnects the worker.
pub(super) struct Keys {
    _subscription: crate::ptt::KeyTapGuard,
}

pub(super) fn listen_for_keys(
    started: Instant,
    inputs: mpsc::UnboundedSender<TeacherInput>,
) -> Option<Keys> {
    let (subscription, received) = crate::ptt::subscribe_keys()?;
    std::thread::Builder::new()
        .name("mm-keys".into())
        .spawn(move || {
            let mut recognizer = ChordRecognizer::new();
            while let Ok(event) = received.recv() {
                if event.at < started {
                    continue;
                }
                let now_ms =
                    u64::try_from(event.at.duration_since(started).as_millis()).unwrap_or(u64::MAX);
                if let Some(input) = recognizer.key_in_window(
                    event.vk,
                    event.down,
                    event.injected,
                    now_ms,
                    event.foreground,
                ) && inputs.send(input).is_err()
                {
                    return;
                }
            }
        })
        .ok()?;
    Some(Keys {
        _subscription: subscription,
    })
}
