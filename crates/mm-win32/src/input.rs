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
    HumanConfirmation, InputAction, InputInjector, MouseButton, PlatformError, TargetWindow,
    check_confirmation,
};
use uiautomation::UIAutomation;
use uiautomation::types::Point;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// How many levels above the element under the point the blocklist reads: an icon or a
/// text inside a button has no label of its own.
const MAX_ANCESTORS: usize = 6;

/// Control types whose names are window or page titles, not what a click does: the walk up
/// stops below them.
const CONTAINERS: [&str; 3] = ["Window", "Pane", "Document"];

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
        check_confirmation(target, action, confirmation)?;
        // Only a left click so far: anything else is refused, never approximated.
        let at = match action {
            InputAction::Click {
                at,
                button: MouseButton::Left,
            } => *at,
            _ => return Err(PlatformError::SafetyBlocked),
        };
        let absolute = virtual_screen()
            .and_then(|screen| normalize(at, screen))
            .ok_or(PlatformError::WindowNotFound)?;
        check_element(target, at)?;
        // Last, right before the click (the UIA reads above can take a while): the window
        // the person said yes in is still in front and the point is inside its frame.
        let front = crate::window::foreground_target(&[]).ok_or(PlatformError::WindowNotFound)?;
        if front.window != target || !front.frame.contains(at) {
            return Err(PlatformError::WindowNotFound);
        }
        click(absolute)
    }
}

/// Refuses unless the element under `at` belongs to the target's process (no other window
/// over the point, Mate included), something on the way up names what the click does, and
/// neither it nor the controls around it are a password field or on the blocklist
/// (`forbidden`, invariant 4). A provider that does not answer means no click.
fn check_element(target: TargetWindow, at: PhysPoint) -> Result<(), PlatformError> {
    let unanswered = |_| PlatformError::UiaTimeout;
    let automation = UIAutomation::new().map_err(unanswered)?;
    let walker = automation.get_control_view_walker().map_err(unanswered)?;
    let mut element = automation
        .element_from_point(Point::new(at.x, at.y))
        .map_err(unanswered)?;
    if element.get_process_id().map_err(unanswered)? != target.process_id {
        return Err(PlatformError::WindowNotFound);
    }
    let mut named = false;
    for level in 0..=MAX_ANCESTORS {
        // Unknown fails closed, like the snapshot walker; a password box's inner text or
        // reveal icon counts as the box.
        if element.is_password().unwrap_or(true) {
            return Err(PlatformError::SafetyBlocked);
        }
        let control_type = format!("{:?}", element.get_control_type().map_err(unanswered)?);
        let name = element.get_name().map_err(unanswered)?;
        let labels = [
            name.clone(),
            element.get_automation_id().map_err(unanswered)?,
            element.get_classname().unwrap_or_default(),
            element.get_help_text().unwrap_or_default(),
        ];
        // A container's own title counts too ("Checkout - Shop"): on such a page Mate
        // presses nothing, the person does.
        if labels.iter().any(|label| forbidden(label, &control_type)) {
            return Err(PlatformError::SafetyBlocked);
        }
        let container = CONTAINERS.contains(&control_type.as_str());
        if level > 0 && container {
            break;
        }
        named |= !name.trim().is_empty();
        if level < MAX_ANCESTORS {
            element = walker.get_parent(&element).map_err(unanswered)?;
        }
    }
    // An icon with no name anywhere below its container: what it does is unknown (a bin
    // icon deletes), so Mate does not press it.
    if !named {
        return Err(PlatformError::SafetyBlocked);
    }
    Ok(())
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
