//! "What's this?" (WP124): the person drags Mate onto a control and lets go. Nothing leaves
//! the PC on the drag or the drop: UI Automation reads what is under the pointer here, the
//! highlight rings it and Mate's bubble offers to explain it for `OFFER_OPEN`. Only a click
//! on Mate while the offer is open asks: one text-only question on the chat route
//! (voice.rs) carrying the control's name, its type and the app's name, with the same
//! sign-in gate and guest count as any other question. Mate then steps aside with his beam
//! on the control while he answers. A click with no offer open starts talking, as before
//! (drag.rs, ptt.rs).
//!
//! Never offered (fail closed): our own windows, nothing a person acts on (the empty
//! desktop), unnamed controls, password fields and anything `privacy::forbidden` refuses on
//! the way up to the window, its title included (as teacher/presentation.rs).

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mm_core::geom::{PhysPoint, PhysRect};
use mm_core::placement::place_mate;
use mm_platform::UiElement;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use crate::answer_stream::AnswerStream;
use crate::flight::Flight;

/// How long the offer stays open after the drop.
const OFFER_OPEN: Duration = Duration::from_secs(6);
/// Levels above the element read for the blocklist, up to the first window: a walk's depth
/// limit (uia.rs `WalkLimits`), since web pages nest deeper than the click check's 12.
const MAX_ANCESTORS: usize = 32;
/// A longer name is content (a message, a paragraph), not a control's label.
const MAX_NAME_CHARS: usize = 80;
/// Clearance between Mate and the control, in CSS pixels at 100 % (as pointing.rs).
const GAP: f64 = 24.0;
/// As voice.rs: the beam stays while the answer plays and a moment after, at most this long.
const LINGER: Duration = Duration::from_secs(3);
const PLAY_LIMIT: Duration = Duration::from_secs(90);
/// The event Mate's window shows the offer for (src/mate.ts).
const OFFER_EVENT: &str = "explain-offer";

/// What a drop found: all the question may carry, and where to shine. No `Debug`: the
/// name can be private text (convention 3).
struct Control {
    name: String,
    control_type: String,
    app: String,
    rect: PhysRect,
}

/// The open offer, if any. `epoch` moves on whenever something newer starts, so a drop
/// still being read is never offered after it.
struct Offers {
    epoch: u64,
    open: Option<(u64, Control, Instant)>,
}

impl Offers {
    /// Ends any open offer (true when one was open) and outdates every drop being read.
    fn withdraw(&mut self) -> bool {
        self.epoch = self.epoch.wrapping_add(1);
        self.open.take().is_some()
    }

    /// Opens the offer of the drop read since `epoch`, unless anything newer came first.
    fn open(&mut self, epoch: u64, control: Control, now: Instant) -> bool {
        if epoch != self.epoch {
            return false;
        }
        self.open = Some((epoch, control, now));
        true
    }

    /// The control of an offer still open at `now`, taken: one click asks once. An expired
    /// offer stays for `expire`, which takes its ring down.
    fn take(&mut self, now: Instant) -> Option<Control> {
        let (_, _, offered) = self.open.as_ref()?;
        if !still_open(*offered, now) {
            return None;
        }
        self.open.take().map(|(_, control, _)| control)
    }

    /// Ends the offer of `epoch` if it is still the open one; true when it was.
    fn expire(&mut self, epoch: u64) -> bool {
        if self.open.as_ref().is_some_and(|(open, ..)| *open == epoch) {
            self.open = None;
            return true;
        }
        false
    }
}

static OFFERS: Mutex<Offers> = Mutex::new(Offers {
    epoch: 0,
    open: None,
});

fn offers() -> MutexGuard<'static, Offers> {
    OFFERS.lock().unwrap_or_else(PoisonError::into_inner)
}

fn still_open(offered: Instant, now: Instant) -> bool {
    now.saturating_duration_since(offered) < OFFER_OPEN
}

/// A new question, press or line (voice.rs) ends the offer; the voice loop clears the ring.
pub(crate) fn withdraw() {
    offers().withdraw();
}

/// Mate let go at `at` after a drag (drag.rs, on the UI thread): what is there is read off
/// this thread and offered. A drop on nothing explainable (or on a private name) does nothing.
pub(crate) fn dropped(app: &AppHandle, at: PhysPoint) {
    let (was_open, epoch) = {
        let mut state = offers();
        (state.withdraw(), state.epoch)
    };
    if was_open {
        let _ = crate::highlight::clear(app);
    }
    // Mate himself is what is under the pointer, unless the work area's edge held him back.
    let covered = app
        .try_state::<Flight>()
        .is_none_or(|flight| flight.position().contains(at));
    let Some(mate) = app
        .get_webview_window("mate")
        .and_then(|window| window.hwnd().ok())
        .map(|hwnd| hwnd.0 as isize)
    else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let hide = covered.then_some(mate);
        // Nothing explainable there (or a name that must not leave the PC): Mate was only
        // moved, so he says nothing.
        let Ok(Some(control)) = tokio::task::spawn_blocking(move || probe(hide, at)).await else {
            return;
        };
        offer(&app, epoch, control);
    });
}

/// What is under `at` and may be offered (blocking: UI Automation, never on the UI thread).
/// `hide` is Mate's window, hidden for the hit test alone; reading the ancestors after it
/// can take longer, and he is back by then.
fn probe(hide: Option<isize>, at: PhysPoint) -> Option<Control> {
    let walker = mm_win32::uia::UiaWalker::new().ok()?;
    let hit = {
        let _hidden = hide.map(Hidden::new);
        walker.hit(at)
    }?;
    let (process_id, chain) = walker.ancestry(&hit, MAX_ANCESTORS)?;
    let element = explainable(&chain, process_id, std::process::id(), at)?;
    Some(Control {
        name: element.name.trim().to_owned(),
        control_type: element.control_type.clone(),
        app: mm_win32::window::process_name(process_id)?,
        rect: element.rect?,
    })
}

/// Mate out of the way of one hit test (as screen.rs hides him for a copy): SetWindowPos
/// with SWP_NOACTIVATE only, so his window keeps its no-activate styles (invariant 1). He
/// comes back unless he was hidden already or went to sleep meanwhile.
struct Hidden(Option<isize>);

impl Hidden {
    fn new(mate: isize) -> Self {
        let hwnd = mm_win32::HWND(mate as *mut std::ffi::c_void);
        if mm_win32::overlay::is_hidden(hwnd) {
            return Self(None);
        }
        mm_win32::overlay::hide_for_capture(hwnd);
        Self(Some(mate))
    }
}

impl Drop for Hidden {
    fn drop(&mut self) {
        if let Some(mate) = self.0
            && !crate::presence::is_hidden(mate)
        {
            mm_win32::overlay::show_after_capture(mm_win32::HWND(mate as *mut std::ffi::c_void));
        }
    }
}

/// The element a drop on `chain` (the element under `at`, then its ancestors up to the
/// window) offers to explain, or None: one of our windows, nothing a person acts on (the
/// desktop's empty space, a pane, a text), no name or a long one, or a password field or a
/// blocked label anywhere on the way up, the window's title included. An icon or a label
/// inside a button stands for the button.
fn explainable(
    chain: &[UiElement],
    process_id: u32,
    own_process: u32,
    at: PhysPoint,
) -> Option<&UiElement> {
    if process_id == 0 || process_id == own_process {
        return None;
    }
    let first = chain.first()?;
    let target = if mm_platform::is_interactive_type(&first.control_type) {
        first
    } else {
        chain.get(1).filter(|parent| {
            mm_platform::can_borrow_click_name(&first.control_type, &parent.control_type)
        })?
    };
    // Page titles and content rows are private content, not control labels.
    // Fields a person types into are never offered: a revealed password is a plain edit whose
    // "Password" label is a sibling, not an ancestor (D37), and some fields name themselves
    // after what was typed. Failing closed costs little: buttons and icons are the point.
    if matches!(
        target.control_type.to_ascii_lowercase().as_str(),
        "edit"
            | "document"
            | "combobox"
            | "spinner"
            | "tabitem"
            | "window"
            | "dataitem"
            | "titlebar"
    ) {
        return None;
    }
    let name = target.name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || mm_core::privacy::private_explain_name(name)
        || target.offscreen
        || !target.rect.is_some_and(|rect| rect.contains(at))
    {
        return None;
    }
    chain
        .iter()
        .all(|element| {
            !element.is_password
                && !element.password_like
                && !mm_core::privacy::forbidden(&element.name, &element.control_type)
        })
        .then_some(target)
}

/// Opens the offer of the drop read since `epoch`: the ring (Mate stays where he was let
/// go, so no beam yet) and the bubble, until `OFFER_OPEN` is up.
fn offer(app: &AppHandle, epoch: u64, control: Control) {
    let (name, rect) = (control.name.clone(), control.rect);
    if !offers().open(epoch, control, Instant::now()) {
        return;
    }
    if !shine(app, rect, false) {
        offers().expire(epoch);
        return;
    }
    let ms = u64::try_from(OFFER_OPEN.as_millis()).unwrap_or(u64::MAX);
    let payload = serde_json::json!({ "control": name, "ms": ms });
    if app.emit_to("mate", OFFER_EVENT, payload).is_err() {
        eprintln!("explain: the offer did not reach Mate's window");
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(OFFER_OPEN).await;
        if offers().expire(epoch) {
            let _ = crate::highlight::clear(&app);
        }
    });
}

/// The ring on `control` on the monitor that holds it; with `aside`, Mate first flies
/// beside it and his beam shines on it (as pointing.rs). False when nothing was shown.
fn shine(app: &AppHandle, control: PhysRect, aside: bool) -> bool {
    let monitors = mm_win32::monitors::monitors();
    let Some((index, monitor)) = monitors
        .iter()
        .enumerate()
        .find(|(_, monitor)| monitor.area.contains(control.center()))
    else {
        return false;
    };
    let scale = monitor.scale();
    let size = (crate::overlay::MATE_SIZE * scale).round() as i32;
    let gap = (GAP * scale).round() as i32;
    let mate = aside
        .then(|| place_mate(control, monitor.work_area, size, size, gap))
        .flatten()
        .map(|placement| placement.mate);
    if let (Some(mate), Some(flight)) = (mate, app.try_state::<Flight>()) {
        flight.fly_to(mate);
    }
    crate::highlight::show(app, index, monitor, control, mate, None).is_ok()
}

/// A click on Mate (drag.rs, on the UI thread): true when it asked about the open offer's
/// control; false when no offer is open, and the click starts talking as before.
pub(crate) fn clicked(app: &AppHandle) -> bool {
    let Some(control) = offers().take(Instant::now()) else {
        return false;
    };
    let locale = app.state::<AnswerStream>().locale();
    let (started, generation) = oneshot::channel();
    if !crate::voice::ask_explain(app, question(&locale, &control), started) {
        let _ = crate::highlight::clear(app);
        return false;
    }
    tauri::async_runtime::spawn(explain(app.clone(), control.rect, generation));
    true
}

/// The only text that leaves the PC: the control's name, its UI Automation type and the
/// app's name, in one pass (a name holding "{app}" stays as it is).
fn question(locale: &str, control: &Control) -> String {
    let template = crate::locale_text(locale, "mate.explain_question");
    let values = [
        ("{control}", control.name.as_str()),
        ("{type}", control.control_type.as_str()),
        ("{app}", control.app.as_str()),
    ];
    let mut text = String::with_capacity(template.len() + control.name.len());
    let mut rest = template.as_str();
    while let Some(start) = rest.find('{') {
        text.push_str(&rest[..start]);
        let tail = &rest[start..];
        match values.iter().find(|(key, _)| tail.starts_with(key)) {
            Some((key, value)) => {
                text.push_str(value);
                rest = &tail[key.len()..];
            }
            None => {
                text.push('{');
                rest = &tail[1..];
            }
        }
    }
    text.push_str(rest);
    text
}

/// Once the voice loop has started the answer (and cleared any earlier ring), Mate steps
/// aside with his beam on the control while he explains, then flies home. A newer press
/// cancels this answer and clears the screen itself; a refused question (sign-in) never
/// starts, and its ring is already gone.
async fn explain(app: AppHandle, control: PhysRect, generation: oneshot::Receiver<u64>) {
    let Ok(generation) = generation.await else {
        return;
    };
    let stream = app.state::<AnswerStream>();
    if stream.is_cancelled(generation) || !shine(&app, control, true) {
        return;
    }
    let asked = Instant::now();
    while stream.reported("done", generation).is_none() && asked.elapsed() < PLAY_LIMIT {
        if stream.is_cancelled(generation) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    tokio::time::sleep(LINGER).await;
    if !stream.is_cancelled(generation) {
        crate::pointing::stop_pointing(&app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: PhysPoint = PhysPoint { x: 10, y: 10 };
    const OURS: u32 = 7;
    const THEIRS: u32 = 42;

    fn element(id: &str, control_type: &str, name: &str) -> UiElement {
        UiElement {
            id: id.into(),
            parent: None,
            control_type: control_type.into(),
            name: name.into(),
            rect: PhysRect::new(0, 0, 100, 40).ok(),
            interactive: mm_platform::is_interactive_type(control_type),
            enabled: true,
            offscreen: false,
            is_password: false,
            password_like: false,
        }
    }

    /// `target` in a Settings page, as the ancestry walk reads it.
    fn in_settings(target: UiElement) -> Vec<UiElement> {
        vec![
            target,
            element("p1", "Pane", "Network & internet"),
            element("p2", "Window", "Settings"),
        ]
    }

    fn picked(chain: &[UiElement], process_id: u32) -> Option<&str> {
        explainable(chain, process_id, OURS, AT).map(|element| element.id.as_str())
    }

    fn control(name: &str) -> Control {
        Control {
            name: name.into(),
            control_type: "Button".into(),
            app: "SystemSettings".into(),
            rect: PhysRect::new(0, 0, 100, 40).unwrap(),
        }
    }

    #[test]
    fn an_offer_is_open_for_six_seconds_and_asks_once() {
        let now = Instant::now();
        let mut offers = Offers {
            epoch: 0,
            open: None,
        };
        assert!(offers.take(now).is_none());
        let epoch = offers.epoch;
        assert!(offers.open(epoch, control("Wi-Fi"), now));
        let taken = offers.take(now + Duration::from_millis(5_999));
        assert!(taken.is_some_and(|taken| taken.name == "Wi-Fi"));
        assert!(offers.take(now).is_none());
        // Taken by the click: its timer must not take the answer's ring down.
        assert!(!offers.expire(epoch));

        assert!(offers.open(epoch, control("Bluetooth"), now));
        assert!(offers.take(now + OFFER_OPEN).is_none());
        // Too late for a click, still there for its timer, which ends it once.
        assert!(offers.expire(epoch));
        assert!(!offers.expire(epoch));
        assert!(offers.take(now).is_none());
    }

    #[test]
    fn the_offer_window_is_six_seconds_from_the_drop() {
        let offered = Instant::now();
        assert_eq!(OFFER_OPEN, Duration::from_secs(6));
        assert!(still_open(offered, offered));
        assert!(still_open(offered, offered + Duration::from_millis(5_999)));
        assert!(!still_open(offered, offered + OFFER_OPEN));
        assert!(!still_open(offered, offered + Duration::from_secs(60)));
        // A clock read just before the drop is no later than the drop.
        assert!(still_open(offered + Duration::from_millis(1), offered));
    }

    #[test]
    fn anything_newer_outdates_a_drop_still_being_read() {
        let now = Instant::now();
        let mut offers = Offers {
            epoch: 0,
            open: None,
        };
        // A press while the drop is being read: that drop is never offered.
        let reading = offers.epoch;
        assert!(!offers.withdraw());
        assert!(!offers.open(reading, control("Wi-Fi"), now));
        assert!(offers.take(now).is_none());

        // A second drop ends the first offer; the first one's timer leaves it alone.
        let first = offers.epoch;
        assert!(offers.open(first, control("Wi-Fi"), now));
        assert!(offers.withdraw());
        let second = offers.epoch;
        assert!(offers.open(second, control("Bluetooth"), now));
        assert!(!offers.expire(first));
        assert!(
            offers
                .take(now)
                .is_some_and(|taken| taken.name == "Bluetooth")
        );

        // A press ends an open offer: the next click talks.
        assert!(offers.open(offers.epoch, control("Wi-Fi"), now));
        assert!(offers.withdraw());
        assert!(offers.take(now).is_none());
    }

    #[test]
    fn a_named_control_in_another_app_is_offered() {
        let chain = in_settings(element("p0", "Button", "Wi-Fi"));
        assert_eq!(picked(&chain, THEIRS), Some("p0"));
        // A desktop icon: the desktop is a list in a pane, without a window.
        let icon = [
            element("p0", "ListItem", "Recycle Bin"),
            element("p1", "List", "Desktop"),
            element("p2", "Pane", "Program Manager"),
        ];
        assert_eq!(picked(&icon, THEIRS), Some("p0"));
    }

    #[test]
    fn our_windows_and_unknown_processes_are_skipped() {
        let chain = in_settings(element("p0", "Button", "Wi-Fi"));
        assert_eq!(picked(&chain, OURS), None);
        assert_eq!(picked(&chain, 0), None);
    }

    #[test]
    fn empty_desktop_panes_and_text_are_skipped() {
        let empty = [
            element("p0", "List", "Desktop"),
            element("p1", "Pane", "Program Manager"),
        ];
        assert_eq!(picked(&empty, THEIRS), None);
        assert_eq!(picked(&empty[1..], THEIRS), None);
        assert_eq!(picked(&[], THEIRS), None);
        let paragraph = in_settings(element("p0", "Text", "Connect to a network"));
        assert_eq!(picked(&paragraph, THEIRS), None);
    }

    #[test]
    fn an_icon_inside_a_button_stands_for_the_button() {
        let chain = [
            element("p0", "Image", ""),
            element("p1", "Button", "Wi-Fi"),
            element("p2", "Window", "Settings"),
        ];
        assert_eq!(picked(&chain, THEIRS), Some("p1"));
        let loose = in_settings(element("p0", "Image", "Wi-Fi"));
        assert_eq!(picked(&loose, THEIRS), None);
    }

    #[test]
    fn page_titles_and_content_targets_are_never_offered() {
        for kind in ["TabItem", "Window", "Document", "DataItem", "TitleBar"] {
            assert_eq!(
                picked(&in_settings(element("p0", kind, "Overview")), THEIRS),
                None
            );
            for child in ["Image", "Text", "Custom"] {
                let chain = [
                    element("p0", child, ""),
                    element("p1", kind, "Overview"),
                    element("p2", "Window", "Browser"),
                ];
                assert_eq!(picked(&chain, THEIRS), None, "{child} in {kind}");
            }
        }
    }

    #[test]
    fn private_names_are_skipped_on_controls_and_borrowed_buttons() {
        for name in [
            "Inbox - person@example.test",
            "Contact @ support",
            "482916",
            "Your sign-in code is 482916",
            "Call 5551234",
            "Balance 1234",
            "x1234y",
            "https://example.test/private",
            "www.example.test",
            "Open example.test/path",
            "example.test",
        ] {
            for kind in ["Button", "ListItem", "Hyperlink"] {
                assert_eq!(
                    picked(&in_settings(element("p0", kind, name)), THEIRS),
                    None,
                    "{kind}: {name}"
                );
            }
            for child in ["Image", "Text", "Custom"] {
                let chain = [element("p0", child, ""), element("p1", "Button", name)];
                assert_eq!(picked(&chain, THEIRS), None, "{child}: {name}");
            }
        }
        for name in ["Wi-Fi", "Print", "Step 123", "Zoom 100%"] {
            assert_eq!(
                picked(&in_settings(element("p0", "Button", name)), THEIRS),
                Some("p0")
            );
        }
    }

    #[test]
    fn unnamed_long_named_or_misplaced_controls_are_skipped() {
        for name in ["", "   "] {
            assert_eq!(
                picked(&in_settings(element("p0", "Button", name)), THEIRS),
                None
            );
        }
        let label = "a".repeat(MAX_NAME_CHARS);
        assert_eq!(
            picked(&in_settings(element("p0", "ListItem", &label)), THEIRS),
            Some("p0")
        );
        let message = "a".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(
            picked(&in_settings(element("p0", "ListItem", &message)), THEIRS),
            None
        );

        let mut elsewhere = element("p0", "Button", "Wi-Fi");
        elsewhere.rect = PhysRect::new(200, 200, 300, 240).ok();
        assert_eq!(picked(&in_settings(elsewhere), THEIRS), None);
        let mut no_bounds = element("p0", "Button", "Wi-Fi");
        no_bounds.rect = None;
        assert_eq!(picked(&in_settings(no_bounds), THEIRS), None);
        let mut offscreen = element("p0", "Button", "Wi-Fi");
        offscreen.offscreen = true;
        assert_eq!(picked(&in_settings(offscreen), THEIRS), None);
    }

    #[test]
    fn password_fields_are_skipped_even_when_revealed() {
        assert_eq!(
            picked(&in_settings(element("p0", "Edit", "Password")), THEIRS),
            None
        );
        assert_eq!(
            picked(&in_settings(element("p0", "Edit", "PIN")), THEIRS),
            None
        );
        let mut flagged = element("p0", "Edit", "Enter here");
        flagged.is_password = true;
        assert_eq!(picked(&in_settings(flagged), THEIRS), None);
        // A revealed password is plain text: only the label heuristic still knows (D37).
        let mut revealed = element("p0", "Edit", "Enter here");
        revealed.password_like = true;
        assert_eq!(picked(&in_settings(revealed), THEIRS), None);
        let mut inside = in_settings(element("p0", "Button", "Show"));
        inside[1].password_like = true;
        assert_eq!(picked(&inside, THEIRS), None);
        // Any field one types into stays out, even a harmless-looking one: a revealed
        // password's "Password" label is its sibling, which no ancestor check sees.
        for control_type in ["Edit", "Document", "ComboBox", "Spinner"] {
            assert_eq!(
                picked(
                    &in_settings(element("p0", control_type, "Enter here")),
                    THEIRS
                ),
                None
            );
        }
        assert_eq!(
            picked(
                &in_settings(element("p0", "Edit", "Search settings")),
                THEIRS
            ),
            None
        );
    }

    #[test]
    fn blocked_controls_and_pages_are_skipped() {
        for name in ["Delete", "Pay now", "Удалить", "Empty Recycle Bin"] {
            assert_eq!(
                picked(&in_settings(element("p0", "Button", name)), THEIRS),
                None,
                "{name}"
            );
        }
        // The window's title counts too, as in the teacher's checks.
        let mut checkout = in_settings(element("p0", "Button", "Continue"));
        checkout[2].name = "Checkout - Shop".into();
        assert_eq!(picked(&checkout, THEIRS), None);
        let mut section = in_settings(element("p0", "Button", "Continue"));
        section[1].name = "Delete your account".into();
        assert_eq!(picked(&section, THEIRS), None);
    }

    #[test]
    fn the_question_carries_only_the_name_type_and_app() {
        let wifi = control("Wi-Fi");
        let en = question("en", &wifi);
        assert_eq!(
            en,
            "In two short sentences, what is \"Wi-Fi\" (Button) for in the SystemSettings app?"
        );
        let ru = question("ru", &wifi);
        assert!(ru.contains("«Wi-Fi»") && ru.contains("Button") && ru.contains("SystemSettings"));
        for text in [&en, &ru] {
            for placeholder in ["{control}", "{type}", "{app}"] {
                assert!(!text.contains(placeholder), "{text}");
            }
        }
        // A name is never read as a placeholder.
        let odd = control("{app} {type} {");
        assert!(question("en", &odd).contains("\"{app} {type} {\" (Button)"));
    }
}
