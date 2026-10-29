//! SAM 3.1 fallback outline for windows with a flat or empty UI Automation tree
//! (docs/prompts.md §4, docs/pointing-risks.md).

use std::time::{Duration, Instant};

use mm_core::api::sam::{SamBox, parse_sam_boxes, pick_sam_box, sam_point_request};
use mm_core::geom::{PhysRect, ShotRect};
use mm_core::redaction::overlaps_any;
use mm_platform::UiSnapshot;
use tokio::time::timeout;

use crate::proxy::{CallKind, Proxy};
use crate::screen::Look;

/// A cold SAM container can take ~82 s (`mm probe sam`); a warm call takes ~3.3 s.
const SAM_TIMEOUT: Duration = Duration::from_secs(6);
/// Minimum count of named, enabled, on-screen interactive elements with bounds before a
/// UI Automation tree counts as non-flat (`docs/pointing-risks.md`).
const MIN_NAMED_INTERACTIVE: usize = 3;

/// Whether `snapshot` has fewer than 3 named, enabled, on-screen interactive elements
/// with valid bounds (`docs/pointing-risks.md`).
pub fn tree_is_flat(snapshot: &UiSnapshot) -> bool {
    snapshot
        .elements
        .iter()
        .filter(|e| {
            e.interactive
                && e.enabled
                && !e.offscreen
                && !e.name.trim().is_empty()
                && e.rect.is_some_and(|r| r.left < r.right && r.top < r.bottom)
        })
        .take(MIN_NAMED_INTERACTIVE)
        .count()
        < MIN_NAMED_INTERACTIVE
}

/// Whether SAM may outline what lies under screenshot pixel `(x, y)`: the whole tree is
/// flat, or the element under the pixel is a container (a canvas page, a Qt window) with
/// no interactive element inside it. A blank spot between the buttons of a rich tree
/// stays "I can't find it" (invariant 5). Window chrome (caption buttons, an address bar)
/// does not make a canvas page "rich".
pub fn outline_allowed(look: &Look, x: u32, y: u32) -> bool {
    let snapshot = &look.snapshot;
    if snapshot.unclassifiable() {
        return false;
    }
    let Some(phys) = mm_core::geom::ShotPoint::new(f64::from(x) + 0.5, f64::from(y) + 0.5)
        .ok()
        .and_then(|shot| look.redacted.frame.shot_to_phys_checked(shot))
    else {
        return false;
    };
    // Black in the picture: nothing there to outline, and nothing to ask about.
    if mm_core::redaction::point_in_any(phys, &look.painted) {
        return false;
    }
    if tree_is_flat(snapshot) {
        return true;
    }
    let Some(under) = snapshot.element_at(phys) else {
        return false;
    };
    // The window itself always holds its chrome: never a reason on its own.
    if under.parent.is_none() || under.interactive {
        return false;
    }
    !snapshot.elements.iter().any(|element| {
        element.interactive
            && element.enabled
            && !element.offscreen
            && element.rect.is_some()
            && descends_from(snapshot, element, &under.id)
    })
}

/// Whether `element` lies below the element `ancestor` (by parent ids; a cycle or a
/// missing parent ends the walk).
fn descends_from(snapshot: &UiSnapshot, element: &mm_platform::UiElement, ancestor: &str) -> bool {
    let mut parent = element.parent.as_deref();
    for _ in 0..snapshot.elements.len() {
        match parent {
            Some(id) if id == ancestor => return true,
            Some(id) => parent = snapshot.element(id).and_then(|up| up.parent.as_deref()),
            None => return false,
        }
    }
    false
}

/// Asks SAM 3.1 through `proxy` for the contour box around screenshot pixel `(x, y)` of
/// `look`, returning its physical screen rectangle when a valid box arrives within 6 s,
/// lies inside `look.snapshot.frame`, and does not touch `look.painted`.
pub async fn sam_outline(proxy: &Proxy, look: &Look, x: u32, y: u32) -> Option<PhysRect> {
    if look.snapshot.unclassifiable() {
        return None;
    }
    let sent = look.redacted.frame.sent_size();
    if x >= sent.0 || y >= sent.1 {
        return None;
    }
    let body = sam_point_request(x, y, sent, &look.redacted.image);
    let started = Instant::now();
    let mut text = String::new();
    let outcome = timeout(
        SAM_TIMEOUT,
        proxy.respond(CallKind::Sam, &body, |delta| text.push_str(delta)),
    )
    .await;
    let elapsed_ms = started.elapsed().as_millis();
    match outcome {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            eprintln!("sam: {error} ({elapsed_ms} ms)");
            return None;
        }
        Err(_) => {
            eprintln!("sam: timed out after {elapsed_ms} ms");
            return None;
        }
    }
    let boxes = parse_sam_boxes(&text, sent);
    let picked = pick_sam_box(&boxes, x, y, sent);
    let rect = picked.and_then(|b| outline_to_phys(look, b));
    #[cfg(debug_assertions)]
    eprintln!(
        "sam: {}x{}, {} chars, {} boxes, picked={}, accepted={} in {elapsed_ms} ms",
        sent.0,
        sent.1,
        text.len(),
        boxes.len(),
        picked.is_some(),
        rect.is_some(),
    );
    rect
}

fn outline_to_phys(look: &Look, sam_box: SamBox) -> Option<PhysRect> {
    let shot = ShotRect::new(
        f64::from(sam_box.left),
        f64::from(sam_box.top),
        f64::from(sam_box.right),
        f64::from(sam_box.bottom),
    )
    .ok()?;
    let mapped = look.redacted.frame.shot_rect_to_phys(shot);
    let phys = PhysRect::new(mapped.left, mapped.top, mapped.right, mapped.bottom).ok()?;
    let frame = look.snapshot.frame;
    if phys.left < frame.left
        || phys.top < frame.top
        || phys.right > frame.right
        || phys.bottom > frame.bottom
        || overlaps_any(phys, &look.painted)
    {
        return None;
    }
    Some(phys)
}

#[cfg(test)]
mod tests {
    use mm_platform::{CapturedImage, TargetWindow, UiElement};

    use super::*;
    use crate::screen::prepare;

    const FRAME: PhysRect = PhysRect {
        left: 100,
        top: 50,
        right: 500,
        bottom: 350,
    };

    fn el(id: &str, control_type: &str, name: &str) -> UiElement {
        UiElement {
            id: id.into(),
            parent: (id != "e0").then(|| "e0".into()),
            control_type: control_type.into(),
            name: name.into(),
            rect: Some(PhysRect::new(120, 70, 200, 100).unwrap()),
            interactive: mm_platform::is_interactive_type(control_type),
            enabled: true,
            offscreen: false,
            is_password: false,
            password_like: false,
        }
    }

    fn snapshot_with(elements: Vec<UiElement>) -> UiSnapshot {
        UiSnapshot {
            target: TargetWindow {
                handle: 1,
                process_id: 2,
            },
            frame: FRAME,
            elements,
            truncated: false,
        }
    }

    fn canvas_capture() -> CapturedImage {
        CapturedImage::new(400, 300, vec![200; 400 * 300 * 4]).unwrap()
    }

    #[test]
    fn a_canvas_without_controls_may_be_outlined_but_a_rich_area_may_not() {
        let window = |id: &str, kind: &str, name: &str, parent: Option<&str>, rect| UiElement {
            parent: parent.map(str::to_owned),
            rect: Some(rect),
            ..el(id, kind, name)
        };
        let whole = FRAME;
        let canvas = PhysRect::new(100, 100, 500, 350).unwrap();
        let chrome = PhysRect::new(400, 50, 420, 70).unwrap();
        let mut elements = vec![
            window("e0", "Window", "Postcard Maker", None, whole),
            window("e1", "Button", "Minimize", Some("e0"), chrome),
            window("e2", "Button", "Maximize", Some("e0"), chrome),
            window("e3", "Button", "Close", Some("e0"), chrome),
            window("e4", "Document", "Postcard Maker", Some("e0"), canvas),
            // A page's document has something inside it: UIA reads this window.
            window("e5", "Text", "Draw here", Some("e4"), canvas),
        ];
        let look = prepare(snapshot_with(elements.clone()), &canvas_capture(), &[]).unwrap();
        // The capture is the frame at 1:1: pixel (300, 200) is physical (400, 250).
        assert!(!tree_is_flat(&look.snapshot));
        assert!(outline_allowed(&look, 300, 200));
        // A button inside the document: the document is a rich area now.
        elements.push(window(
            "e5",
            "Button",
            "Send",
            Some("e4"),
            PhysRect::new(110, 110, 150, 130).unwrap(),
        ));
        let look = prepare(snapshot_with(elements), &canvas_capture(), &[]).unwrap();
        assert!(!outline_allowed(&look, 300, 200));
        // Above the canvas only the window itself is under the pixel: not allowed.
        assert!(!outline_allowed(&look, 10, 10));
    }

    #[test]
    fn tree_is_flat_requires_at_least_three_named_enabled_onscreen_interactive_elements() {
        assert!(tree_is_flat(&snapshot_with(vec![])));
        assert!(tree_is_flat(&snapshot_with(vec![
            el("e0", "Window", "Telegram"),
            el("e1", "Button", "Minimize"),
            el("e2", "Button", "Close"),
        ])));
        assert!(!tree_is_flat(&snapshot_with(vec![
            el("e0", "Window", "Telegram"),
            el("e1", "Button", "Minimize"),
            el("e2", "Button", "Maximize"),
            el("e3", "Button", "Close"),
        ])));

        let mut unnamed = el("e3", "Button", "   ");
        unnamed.id = "e3".into();
        let mut disabled = el("e4", "Button", "Send");
        disabled.enabled = false;
        let mut offscreen = el("e5", "Button", "Attach");
        offscreen.offscreen = true;
        let mut no_bounds = el("e6", "Button", "Emoji");
        no_bounds.rect = None;
        let plain_text = el("e7", "Text", "Saved Messages");

        assert!(tree_is_flat(&snapshot_with(vec![
            el("e0", "Window", "Telegram"),
            el("e1", "Button", "Minimize"),
            el("e2", "Button", "Close"),
            unnamed,
            disabled,
            offscreen,
            no_bounds,
            plain_text,
        ])));
    }

    #[test]
    fn outline_maps_to_physical_pixels_and_refuses_painted_or_out_of_frame_boxes() {
        let capture = CapturedImage::new(400, 300, vec![200; 400 * 300 * 4]).unwrap();
        let occluded = PhysRect::new(350, 250, 500, 350).unwrap();
        let mut look = prepare(
            snapshot_with(vec![el("doc", "Document", "Canvas"), {
                // Something inside the document: UIA reads this window.
                let mut text = el("text", "Text", "Draw here");
                text.parent = Some("doc".into());
                text
            }]),
            &capture,
            &[occluded],
        )
        .unwrap();

        let clear = SamBox {
            left: 20,
            top: 30,
            right: 60,
            bottom: 70,
        };
        assert_eq!(
            outline_to_phys(&look, clear),
            Some(PhysRect::new(120, 80, 160, 120).unwrap())
        );

        let touches_painted = SamBox {
            left: 240,
            top: 190,
            right: 260,
            bottom: 210,
        };
        assert_eq!(outline_to_phys(&look, touches_painted), None);

        look.snapshot.frame = PhysRect::new(130, 50, 500, 350).unwrap();
        assert_eq!(outline_to_phys(&look, clear), None);
    }
}
