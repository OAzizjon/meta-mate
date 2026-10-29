//! "Do it for me" (Day 7, docs/architecture.md section 3 B.5): one left click in the user's
//! app after the person said yes to exactly that click (invariant 4). Every check fails
//! closed: no yes, another window in front, a point outside its frame or over another
//! window, or a control on the blocklist means no click. Nothing is restored afterwards and
//! no window is activated: invariant 1 is about our windows, and the click lands in the
//! user's app because the user asked for it. Our mouse hook sees it flagged as injected,
//! so it never completes a step.
//!
//! Call it off the UI thread: the blocklist check puts the calling thread into the COM
//! multithreaded apartment, like `uia::UiaWalker`.

use mm_core::geom::{PhysPoint, PhysRect};
use mm_core::privacy::forbidden;
use mm_platform::{InputAction, InputInjector, MouseButton, PlatformError, TargetWindow};
use uiautomation::types::{Handle, Point};
use uiautomation::{UIAutomation, UIElement};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK,
    MOUSEINPUT, SendInput, VK_CONTROL, VK_LBUTTON, VK_LWIN, VK_MBUTTON, VK_MENU, VK_RBUTTON,
    VK_RWIN, VK_SHIFT, VK_XBUTTON1, VK_XBUTTON2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetForegroundWindow, GetSystemMetrics, GetWindowThreadProcessId,
    IsHungAppWindow, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_SWAPBUTTON, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, WindowFromPoint,
};

/// How many levels above the element under the point the blocklist reads, up to the first
/// window: an icon inside a button has no label of its own, and a dialog's title says what
/// its "Yes" does.
const MAX_ANCESTORS: usize = 12;

/// Performs one confirmed left click with `SendInput`; every other action is refused.
#[derive(Debug, Default, Clone, Copy)]
pub struct Win32Injector;

impl InputInjector for Win32Injector {
    type Confirmation = HumanConfirmation;
    type Offer = PendingConfirmation;

    fn ask_confirmation(
        &mut self,
        target: TargetWindow,
        action: &InputAction,
    ) -> Result<PendingConfirmation, PlatformError> {
        let InputAction::Click {
            at,
            button: MouseButton::Left,
        } = action
        else {
            return Err(PlatformError::SafetyBlocked);
        };
        let element = confirmation_element(target, *at)?;
        // Read focus after the potentially slow UIA walk, before making the offer.
        let front = crate::window::foreground_target(&[]).ok_or(PlatformError::WindowNotFound)?;
        if front.window != target || !inside_target(*at, front.frame, &front.popups) {
            return Err(PlatformError::WindowNotFound);
        }
        recheck_element(target, front.window, *at, &element, &element)?;
        Ok(PendingConfirmation(HumanConfirmation {
            target,
            foreground: front.window,
            action: action.clone(),
            element,
        }))
    }

    fn execute_verified(
        &mut self,
        target: TargetWindow,
        action: &InputAction,
        confirmation: Option<HumanConfirmation>,
    ) -> Result<(), PlatformError> {
        let confirmed = check_confirmation(target, action, confirmation)?;
        // Only a left click so far: anything else is refused, never approximated.
        let at = match action {
            InputAction::Click {
                at,
                button: MouseButton::Left,
            } => *at,
            _ => return Err(PlatformError::SafetyBlocked),
        };
        let before = virtual_screen().ok_or(PlatformError::WindowNotFound)?;
        let live = confirmation_element(target, at)?;
        // Last, right before the click (the UIA reads above can take a while): the window
        // the person said yes in is still in front and the point is in it or its popups.
        let front = crate::window::foreground_target(&[]).ok_or(PlatformError::WindowNotFound)?;
        if front.window != target || !inside_target(at, front.frame, &front.popups) {
            return Err(PlatformError::WindowNotFound);
        }
        recheck_element(target, front.window, at, &confirmed, &live)?;
        let after = virtual_screen().ok_or(PlatformError::WindowNotFound)?;
        let absolute = stable_coordinates(at, before, after).ok_or(PlatformError::SafetyBlocked)?;
        injection_guard(target, at, &front.popup_hwnds)?;
        click(absolute)
    }
}

fn stable_coordinates(at: PhysPoint, before: PhysRect, after: PhysRect) -> Option<(i32, i32)> {
    (before == after).then(|| normalize(at, after)).flatten()
}

/// Only cheap Win32 reads here: no UIA or window enumeration after this guard.
fn injection_guard(
    target: TargetWindow,
    at: PhysPoint,
    popups: &[HWND],
) -> Result<(), PlatformError> {
    let hwnd = HWND(target.handle as usize as *mut std::ffi::c_void);
    let mut process_id = 0;
    // SAFETY: handles and coordinates are values; the only output pointer is local.
    let safe = unsafe {
        let front = GetForegroundWindow();
        let thread = GetWindowThreadProcessId(front, Some(&mut process_id));
        let hit = WindowFromPoint(POINT { x: at.x, y: at.y });
        let owner = GetAncestor(hit, GA_ROOT);
        front == hwnd
            && thread != 0
            && process_id == target.process_id
            && !hit.is_invalid()
            && !owner.is_invalid()
            && (owner == hwnd || popups.contains(&owner))
            && !IsHungAppWindow(hwnd).as_bool()
            && GetSystemMetrics(SM_SWAPBUTTON) == 0
            && [
                VK_CONTROL,
                VK_SHIFT,
                VK_MENU,
                VK_LWIN,
                VK_RWIN,
                VK_LBUTTON,
                VK_RBUTTON,
                VK_MBUTTON,
                VK_XBUTTON1,
                VK_XBUTTON2,
            ]
            .iter()
            .all(|key| GetAsyncKeyState(i32::from(key.0)) >= 0)
    };
    if safe {
        Ok(())
    } else {
        Err(PlatformError::SafetyBlocked)
    }
}

fn inside_target(at: PhysPoint, frame: PhysRect, popups: &[PhysRect]) -> bool {
    frame.contains(at) || popups.iter().any(|rect| rect.contains(at))
}

/// Refuses unless the element under `at` belongs to the target's process (no other window
/// over the point, Mate included), something on the way up names what the click does, and
/// neither it nor the controls around it are a password field or on the blocklist
/// (`forbidden`, invariant 4). A provider that does not answer means no click.
pub fn confirmation_element(
    target: TargetWindow,
    at: PhysPoint,
) -> Result<ConfirmedElement, PlatformError> {
    let unanswered = |_| PlatformError::UiaTimeout;
    let automation = UIAutomation::new().map_err(unanswered)?;
    let walker = automation.get_control_view_walker().map_err(unanswered)?;
    let desktop = automation.get_root_element().map_err(unanswered)?;
    // Menus may be desktop children, so their ancestry never reads the owner's title.
    let hwnd = HWND(target.handle as usize as *mut std::ffi::c_void);
    let window = automation
        .element_from_handle(Handle::from(hwnd))
        .map_err(unanswered)?;
    if forbidden(&window.get_name().map_err(unanswered)?, "Window") {
        return Err(PlatformError::SafetyBlocked);
    }
    let mut element = automation
        .element_from_point(Point::new(at.x, at.y))
        .map_err(unanswered)?;
    if element.get_process_id().map_err(unanswered)? != target.process_id {
        return Err(PlatformError::WindowNotFound);
    }
    let snapshot = describe_click(&element)?;
    let mut named = false;
    let mut reached_root = false;
    for level in 0..=MAX_ANCESTORS {
        // Unknown fails closed, like the snapshot walker; a password box's inner text or
        // reveal icon counts as the box.
        if element.is_password().unwrap_or(true)
            || !element.is_enabled().map_err(unanswered)?
            || element.is_offscreen().map_err(unanswered)?
        {
            return Err(PlatformError::SafetyBlocked);
        }
        let control_type = format!("{:?}", element.get_control_type().map_err(unanswered)?);
        let name = element.get_name().map_err(unanswered)?;
        let labels = [
            name.clone(),
            element.get_automation_id().map_err(unanswered)?,
            element.get_classname().map_err(unanswered)?,
            element.get_help_text().map_err(unanswered)?,
        ];
        // A container's own title counts too ("Checkout - Shop"): on such a page Mate
        // presses nothing, the person does.
        if labels.iter().any(|label| forbidden(label, &control_type)) {
            return Err(PlatformError::SafetyBlocked);
        }
        // The walk goes on through panes and documents (a "Delete file?" dialog's "Yes"
        // sits in one) and ends with the first window, whose title was just checked.
        if level > 0 && control_type == "Window" {
            reached_root = true;
            break;
        }
        // A container's title names the page, not what the click does.
        if level == 0
            || (level == 1
                && mm_platform::can_borrow_click_name(&snapshot.control_type, &control_type))
        {
            named |= !name.trim().is_empty();
        }
        if level < MAX_ANCESTORS {
            let parent = walker.get_parent(&element).map_err(unanswered)?;
            if automation
                .compare_elements(&parent, &desktop)
                .map_err(unanswered)?
            {
                reached_root = ["Menu", "List", "Pane"].contains(&control_type.as_str());
                break;
            }
            element = parent;
        }
    }
    // An icon with no name anywhere below its container: what it does is unknown (a bin
    // icon deletes), so Mate does not press it. Missing roots and excessive depth also
    // fail closed; a popup's owner title was checked separately above.
    if !named || !reached_root {
        return Err(PlatformError::SafetyBlocked);
    }
    // Walking the ancestors can take time. Hit-test again and read current properties
    // after that walk, so the last read describes what is under the point now.
    let live = automation
        .element_from_point(Point::new(at.x, at.y))
        .map_err(unanswered)?;
    let live = describe_click(&live)?;
    recheck_element(target, target, at, &snapshot, &live)?;
    Ok(live)
}

fn describe_click(element: &UIElement) -> Result<ConfirmedElement, PlatformError> {
    let unanswered = |_| PlatformError::UiaTimeout;
    let bounds = element.get_bounding_rectangle().map_err(unanswered)?;
    let name = element.get_name().map_err(unanswered)?;
    let control_type = format!("{:?}", element.get_control_type().map_err(unanswered)?);
    let labels = [
        name.clone(),
        element.get_automation_id().map_err(unanswered)?,
        element.get_classname().map_err(unanswered)?,
        element.get_help_text().map_err(unanswered)?,
    ];
    Ok(ConfirmedElement {
        runtime_id: element.get_runtime_id().map_err(unanswered)?,
        process_id: element.get_process_id().map_err(unanswered)?,
        blocked: element.is_password().unwrap_or(true)
            || labels.iter().any(|label| forbidden(label, &control_type)),
        name,
        control_type,
        rect: PhysRect::new(
            bounds.get_left(),
            bounds.get_top(),
            bounds.get_right(),
            bounds.get_bottom(),
        )
        .map_err(|_| PlatformError::SafetyBlocked)?,
        enabled: element.is_enabled().map_err(unanswered)?,
        offscreen: element.is_offscreen().map_err(unanswered)?,
    })
}

/// The bounding box of every monitor in physical pixels (the process is Per-Monitor DPI v2
/// aware, invariant 2); `None` when it is empty.
fn virtual_screen() -> Option<PhysRect> {
    // SAFETY: these queries take no pointers, only constant system metric indices.
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    let right = left.checked_add(width)?;
    let bottom = top.checked_add(height)?;
    PhysRect::new(left, top, right, bottom).ok()
}

/// `point` as `SendInput` absolute coordinates over the whole virtual desktop
/// (`MOUSEEVENTF_VIRTUALDESK`): 0..=65535 on each axis, aimed at the pixel's center so
/// Windows' rounding lands on that pixel. `None` outside `screen` or when it is empty.
pub fn normalize(point: PhysPoint, screen: PhysRect) -> Option<(i32, i32)> {
    if !screen.contains(point) {
        return None;
    }
    Some((
        axis(point.x, screen.left, screen.right)?,
        axis(point.y, screen.top, screen.bottom)?,
    ))
}

/// `(offset + 0.5) * 65536 / extent`, rounded down; i64 so no product overflows.
fn axis(value: i32, start: i32, end: i32) -> Option<i32> {
    let offset = i64::from(value) - i64::from(start);
    let extent = i64::from(end) - i64::from(start);
    if offset < 0 || offset >= extent {
        return None;
    }
    i32::try_from((2 * offset + 1) * 65_536 / (2 * extent)).ok()
}

/// The move, press and release as one batch, so no other input lands between them.
fn click(absolute: (i32, i32)) -> Result<(), PlatformError> {
    let batch = click_batch(absolute);
    // SAFETY: `batch` holds fully initialized mouse INPUT structs on our stack and the size passed
    // is the size of one INPUT, as SendInput requires.
    let sent = unsafe { SendInput(&batch, size_of::<INPUT>() as i32) };
    if sent as usize == batch.len() {
        return Ok(());
    }
    // A press without its release would leave the button held down in the user's app.
    if sent == 2 {
        let release = [mouse_input(0, 0, MOUSEEVENTF_LEFTUP)];
        // SAFETY: one fully initialized mouse INPUT on our stack, with the size of one INPUT.
        let _ = unsafe { SendInput(&release, size_of::<INPUT>() as i32) };
    }
    // Nothing inserted: UIPI blocks input into an elevated window, or another thread
    // blocked input.
    Err(PlatformError::WindowNotFound)
}

/// An absolute move over the virtual desktop, then the left button down and up there.
fn click_batch((dx, dy): (i32, i32)) -> [INPUT; 3] {
    [
        mouse_input(
            dx,
            dy,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        ),
        mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN),
        mouse_input(0, 0, MOUSEEVENTF_LEFTUP),
    ]
}

fn mouse_input(dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PhysRect {
        PhysRect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn at(x: i32, y: i32) -> PhysPoint {
        PhysPoint { x, y }
    }

    #[test]
    fn clicks_may_land_in_the_frame_or_a_popup_but_not_the_gap() {
        let frame = rect(0, 0, 100, 100);
        let popups = [rect(120, 10, 160, 50), rect(160, 30, 200, 70)];
        for point in [at(20, 20), at(120, 10), at(199, 69)] {
            assert!(inside_target(point, frame, &popups));
        }
        for point in [at(110, 20), at(200, 50), at(160, 70), at(-1, 0)] {
            assert!(!inside_target(point, frame, &popups));
        }
        assert!(!inside_target(at(120, 10), frame, &[]));
    }

    #[test]
    fn layout_change_during_uia_reads_refuses_coordinates() {
        let before = rect(0, 0, 1920, 1080);
        assert_eq!(
            stable_coordinates(at(1800, 500), before, before),
            normalize(at(1800, 500), before)
        );
        for after in [
            rect(-1920, 0, 1920, 1080),
            rect(0, 0, 3840, 1080),
            rect(0, -200, 1920, 1080),
        ] {
            assert_eq!(stable_coordinates(at(1800, 500), before, after), None);
        }
    }

    /// The pixel Windows maps an absolute coordinate back to, with either divisor seen in
    /// the wild (65536 or 65535).
    fn back(normalized: i32, start: i32, end: i32, divisor: i64) -> i32 {
        let extent = i64::from(end) - i64::from(start);
        let pixel = i64::from(start) + i64::from(normalized) * extent / divisor;
        i32::try_from(pixel).unwrap()
    }

    #[test]
    fn corners_and_center_of_one_monitor() {
        let screen = rect(0, 0, 1920, 1080);
        assert_eq!(normalize(at(0, 0), screen), Some((17, 30)));
        assert_eq!(normalize(at(960, 540), screen), Some((32785, 32798)));
        assert_eq!(normalize(at(1919, 1079), screen), Some((65518, 65505)));
    }

    #[test]
    fn negative_origins_work() {
        // A monitor left of and above the primary one.
        let screen = rect(-1920, -200, 1920, 1080);
        assert_eq!(normalize(at(-1920, -200), screen), Some((8, 25)));
        assert_eq!(normalize(at(0, 0), screen), Some((32776, 10265)));
        assert_eq!(normalize(at(1919, 1079), screen), Some((65527, 65510)));
    }

    #[test]
    fn every_pixel_maps_back_to_itself() {
        for (start, end) in [(0, 1), (0, 7), (0, 1920), (-1920, 1920), (-2560, 5120)] {
            for pixel in start..end {
                let normalized = axis(pixel, start, end).unwrap();
                assert!(
                    (0..=65_535).contains(&normalized),
                    "{pixel} in {start}..{end}"
                );
                for divisor in [65_536, 65_535] {
                    assert_eq!(back(normalized, start, end, divisor), pixel);
                }
            }
        }
    }

    #[test]
    fn points_outside_or_empty_screens_are_refused() {
        let screen = rect(-1920, 0, 1920, 1080);
        for point in [at(1920, 0), at(-1921, 0), at(0, 1080), at(0, -1)] {
            assert_eq!(normalize(point, screen), None);
        }
        assert_eq!(normalize(at(0, 0), rect(0, 0, 0, 1080)), None);
        assert_eq!(normalize(at(0, 0), rect(10, 10, -10, -10)), None);
        assert_eq!(
            normalize(at(i32::MAX - 1, 0), rect(i32::MIN, 0, i32::MAX, 1)),
            Some((65535, 32768))
        );
    }

    #[test]
    fn the_click_is_one_absolute_move_then_down_and_up() {
        let batch = click_batch((100, 200));
        let mouse = |input: &INPUT| {
            assert_eq!(input.r#type, INPUT_MOUSE);
            // SAFETY: every INPUT `click_batch` builds is a mouse input, so `mi` is the
            // initialized field of the union.
            let mi = unsafe { input.Anonymous.mi };
            (mi.dx, mi.dy, mi.dwFlags)
        };
        let moved = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
        assert_eq!(mouse(&batch[0]), (100, 200, moved));
        assert_eq!(mouse(&batch[1]), (0, 0, MOUSEEVENTF_LEFTDOWN));
        assert_eq!(mouse(&batch[2]), (0, 0, MOUSEEVENTF_LEFTUP));
    }
}

/// A human's yes for one exact action in one window. Create it only in the handler of the
/// user's own answer to our confirmation prompt, never from model output or
/// `pending_safety_checks`. It is not `Clone`: one yes, one action.
///
/// Created only by answering a platform prompt. The element captured before that prompt
/// is compared with a live read immediately before injection.
///
/// ```compile_fail
/// use mm_platform::{InputAction, TargetWindow};
/// use mm_win32::input::{ConfirmedElement, HumanConfirmation};
/// fn fabricate(target: TargetWindow, action: InputAction, element: ConfirmedElement) {
///     let _ = HumanConfirmation { target, foreground: target, action, element };
/// }
/// ```
#[derive(Debug)]
pub struct HumanConfirmation {
    target: TargetWindow,
    foreground: TargetWindow,
    action: InputAction,
    element: ConfirmedElement,
}

impl HumanConfirmation {
    pub fn target(&self) -> TargetWindow {
        self.target
    }

    pub fn action(&self) -> &InputAction {
        &self.action
    }
}

/// A UIA hit-test result, including the safety result of reading its ancestors and owner.
/// Identity uses the exact runtime id, name and type plus a rectangle tolerance of three pixels.
///
/// ```compile_fail
/// use mm_win32::input::ConfirmedElement;
/// fn change_safety(element: &mut ConfirmedElement) {
///     element.blocked = false;
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmedElement {
    runtime_id: Vec<i32>,
    process_id: u32,
    name: String,
    control_type: String,
    rect: PhysRect,
    enabled: bool,
    offscreen: bool,
    blocked: bool,
}

impl ConfirmedElement {
    pub fn control_type(&self) -> &str {
        &self.control_type
    }

    /// Snapshot identity at the step's click point; runtime identity is checked separately.
    pub fn matches_target(&self, name: &str, control_type: &str, rect: PhysRect) -> bool {
        let close = |a: i32, b: i32| (i64::from(a) - i64::from(b)).abs() <= 3;
        self.name == name
            && self.control_type == control_type
            && close(self.rect.left, rect.left)
            && close(self.rect.top, rect.top)
            && close(self.rect.right, rect.right)
            && close(self.rect.bottom, rect.bottom)
    }
}

/// A displayed offer, consumed by one answer from the person's speech handler.
#[derive(Debug)]
pub struct PendingConfirmation(HumanConfirmation);

impl PendingConfirmation {
    pub fn element(&self) -> &ConfirmedElement {
        &self.0.element
    }

    /// Never call with model output or a pending safety check.
    pub fn answer(self, yes: bool) -> Option<HumanConfirmation> {
        yes.then_some(self.0)
    }
}

/// Test-only prompt fixture; production offers must go through the injector's live read.
#[cfg(test)]
pub(crate) fn ask_confirmation(
    target: TargetWindow,
    action: &InputAction,
    element: ConfirmedElement,
    show_prompt: impl FnOnce(),
) -> Result<PendingConfirmation, PlatformError> {
    let InputAction::Click {
        at,
        button: MouseButton::Left,
    } = action
    else {
        return Err(PlatformError::SafetyBlocked);
    };
    recheck_element(target, target, *at, &element, &element)?;
    show_prompt();
    Ok(PendingConfirmation(HumanConfirmation {
        target,
        foreground: target,
        action: action.clone(),
        element,
    }))
}

/// Pure final guard. Unknown UIA properties must fail before constructing a snapshot.
pub fn recheck_element(
    target: TargetWindow,
    foreground: TargetWindow,
    at: PhysPoint,
    confirmed: &ConfirmedElement,
    live: &ConfirmedElement,
) -> Result<(), PlatformError> {
    if foreground != target
        || confirmed.process_id != target.process_id
        || live.process_id != target.process_id
    {
        return Err(PlatformError::WindowNotFound);
    }
    for element in [confirmed, live] {
        if !element.enabled
            || element.runtime_id.is_empty()
            || element.offscreen
            || element.blocked
            || forbidden(&element.name, &element.control_type)
            || !element.rect.contains(at)
        {
            return Err(PlatformError::SafetyBlocked);
        }
    }
    if confirmed.runtime_id != live.runtime_id
        || !confirmed.matches_target(&live.name, &live.control_type, live.rect)
    {
        return Err(PlatformError::SafetyBlocked);
    }
    Ok(())
}

/// Refuses unless `confirmation` is the human's yes for exactly `action` in exactly
/// `target`: a yes in one window never carries over to another (invariant 4). Every
/// `InputInjector` calls this before anything else.
pub fn check_confirmation(
    target: TargetWindow,
    action: &InputAction,
    confirmation: Option<HumanConfirmation>,
) -> Result<ConfirmedElement, PlatformError> {
    match confirmation {
        Some(yes) if yes.target == target && yes.foreground == target && yes.action == *action => {
            Ok(yes.element)
        }
        _ => Err(PlatformError::ConfirmationRequired),
    }
}

#[cfg(test)]
fn test_element() -> ConfirmedElement {
    ConfirmedElement {
        runtime_id: vec![1, 42],
        process_id: 42,
        name: "Open".into(),
        control_type: "Button".into(),
        rect: PhysRect::new(0, 0, 40, 40).unwrap(),
        enabled: true,
        offscreen: false,
        blocked: false,
    }
}

#[cfg(test)]
mod confirmation_tests {
    use super::*;
    use mm_platform::can_borrow_click_name;

    const TARGET: TargetWindow = TargetWindow {
        handle: 7,
        process_id: 42,
    };
    const AT: PhysPoint = PhysPoint { x: 20, y: 20 };

    fn check(live: &ConfirmedElement) -> Result<(), PlatformError> {
        recheck_element(TARGET, TARGET, AT, &test_element(), live)
    }

    #[test]
    fn unchanged_element_and_three_pixel_rect_drift_are_allowed() {
        let mut live = test_element();
        assert_eq!(check(&live), Ok(()));
        live.rect = PhysRect::new(3, -3, 43, 37).unwrap();
        assert_eq!(check(&live), Ok(()));
    }

    #[test]
    fn replacement_with_identical_label_and_bounds_is_refused() {
        let mut live = test_element();
        live.runtime_id = vec![1, 43];
        assert_eq!(check(&live), Err(PlatformError::SafetyBlocked));
        live.runtime_id.clear();
        assert_eq!(check(&live), Err(PlatformError::SafetyBlocked));
        assert!(
            ask_confirmation(
                TARGET,
                &InputAction::Click {
                    at: AT,
                    button: MouseButton::Left,
                },
                live,
                || panic!("unknown identity")
            )
            .is_err()
        );
    }

    #[test]
    fn decorative_children_only_borrow_direct_action_parent_names() {
        for child in ["Image", "Text", "Custom"] {
            for parent in ["Button", "MenuItem", "Hyperlink", "SplitButton", "TabItem"] {
                assert!(can_borrow_click_name(child, parent));
            }
            for parent in ["ListItem", "ToolBar", "Pane", "Window"] {
                assert!(!can_borrow_click_name(child, parent));
            }
        }
        assert!(!can_borrow_click_name("Button", "Button"));
    }

    #[test]
    fn changed_name_type_or_any_rect_edge_is_refused() {
        let mut live = test_element();
        live.name = "Close".into();
        assert!(check(&live).is_err());
        live = test_element();
        live.control_type = "MenuItem".into();
        assert!(check(&live).is_err());
        for rect in [
            PhysRect::new(4, 0, 40, 40).unwrap(),
            PhysRect::new(0, 4, 40, 40).unwrap(),
            PhysRect::new(0, 0, 44, 40).unwrap(),
            PhysRect::new(0, 0, 40, 44).unwrap(),
        ] {
            live = test_element();
            live.rect = rect;
            assert!(check(&live).is_err());
        }
    }

    #[test]
    fn changed_foreground_handle_or_process_is_refused() {
        for front in [
            TargetWindow {
                handle: 8,
                ..TARGET
            },
            TargetWindow {
                process_id: 43,
                ..TARGET
            },
        ] {
            assert_eq!(
                recheck_element(TARGET, front, AT, &test_element(), &test_element()),
                Err(PlatformError::WindowNotFound)
            );
        }
        let mut live = test_element();
        live.process_id = 43;
        assert_eq!(check(&live), Err(PlatformError::WindowNotFound));
    }

    #[test]
    fn disabled_offscreen_and_ancestor_blocked_elements_are_refused() {
        for live in [
            ConfirmedElement {
                enabled: false,
                ..test_element()
            },
            ConfirmedElement {
                offscreen: true,
                ..test_element()
            },
            ConfirmedElement {
                blocked: true,
                ..test_element()
            },
        ] {
            assert_eq!(check(&live), Err(PlatformError::SafetyBlocked));
        }
    }

    #[test]
    fn click_point_must_still_be_inside_both_rectangles() {
        let element = test_element();
        assert!(
            recheck_element(
                TARGET,
                TARGET,
                PhysPoint { x: 40, y: 20 },
                &element,
                &element
            )
            .is_err()
        );
        let mut live = element.clone();
        live.rect.left = 21;
        let mut confirmed = live.clone();
        confirmed.rect.left = 18;
        assert!(recheck_element(TARGET, TARGET, AT, &confirmed, &live).is_err());
    }

    #[test]
    fn every_forbidden_category_is_refused_even_with_identical_identity() {
        for (name, kind) in [
            ("Payment", "Button"),
            ("Buy", "Button"),
            ("Delete", "Button"),
            ("Transfer money", "Button"),
            ("Password", "Edit"),
        ] {
            let element = ConfirmedElement {
                name: name.into(),
                control_type: kind.into(),
                ..test_element()
            };
            assert_eq!(
                recheck_element(TARGET, TARGET, AT, &element, &element),
                Err(PlatformError::SafetyBlocked)
            );
        }
    }

    #[test]
    fn platform_prompt_precedes_answer_and_one_yes_is_consumed() {
        let action = InputAction::Click {
            at: AT,
            button: MouseButton::Left,
        };
        let mut shown = false;
        let offer = ask_confirmation(TARGET, &action, test_element(), || shown = true).unwrap();
        assert!(shown);
        let yes = offer.answer(true);
        assert_eq!(check_confirmation(TARGET, &action, yes), Ok(test_element()));
        let no = ask_confirmation(TARGET, &action, test_element(), || {})
            .unwrap()
            .answer(false);
        assert_eq!(
            check_confirmation(TARGET, &action, no),
            Err(PlatformError::ConfirmationRequired)
        );
        let blocked = ConfirmedElement {
            enabled: false,
            ..test_element()
        };
        assert!(ask_confirmation(TARGET, &action, blocked, || panic!("unsafe offer")).is_err());
        assert!(
            ask_confirmation(
                TARGET,
                &InputAction::DoubleClick { at: AT },
                test_element(),
                || panic!("double click")
            )
            .is_err()
        );
    }
}
