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
use mm_platform::{
    ConfirmedElement, HumanConfirmation, InputAction, InputInjector, MouseButton, PlatformError,
    TargetWindow, check_confirmation, recheck_element,
};
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
    GA_ROOTOWNER, GetAncestor, GetForegroundWindow, GetSystemMetrics, GetWindowThreadProcessId,
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
        let owner = GetAncestor(hit, GA_ROOTOWNER);
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
