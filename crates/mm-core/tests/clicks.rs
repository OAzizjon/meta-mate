use mm_core::geom::PhysPoint;
use mm_core::teacher::{ClickRecognizer, TeacherInput};

const AT: PhysPoint = PhysPoint { x: -10, y: 20 };

fn click(recognizer: &mut ClickRecognizer, at: PhysPoint, down_ms: u64) -> Vec<TeacherInput> {
    assert!(recognizer.button(true, true, at, false, down_ms).is_empty());
    recognizer.button(true, false, at, false, down_ms + 1)
}

fn assert_click(input: &TeacherInput, at: PhysPoint, left: bool, injected: bool, now_ms: u64) {
    match input {
        TeacherInput::Click {
            at: actual_at,
            left: actual_left,
            injected: actual_injected,
            now_ms: actual_ms,
        } => assert_eq!(
            (*actual_at, *actual_left, *actual_injected, *actual_ms),
            (at, left, injected, now_ms)
        ),
        other => panic!("expected Click, got {other:?}"),
    }
}

fn assert_double(input: &TeacherInput, at: PhysPoint, injected: bool, now_ms: u64) {
    match input {
        TeacherInput::DoubleClick {
            at: actual_at,
            left,
            injected: actual_injected,
            now_ms: actual_ms,
        } => assert_eq!(
            (*actual_at, *left, *actual_injected, *actual_ms),
            (at, true, injected, now_ms)
        ),
        other => panic!("expected DoubleClick, got {other:?}"),
    }
}

#[test]
fn single_click_uses_up_position_and_time() {
    let mut recognizer = ClickRecognizer::new(500, 4, 6);
    assert!(recognizer.button(true, true, AT, false, 10).is_empty());
    let up = PhysPoint { x: 100, y: 200 };
    let out = recognizer.button(true, false, up, false, 30);
    assert_eq!(out.len(), 1);
    assert_click(&out[0], up, true, false, 30);
}

#[test]
fn double_click_uses_down_time_and_position() {
    let mut recognizer = ClickRecognizer::new(500, 4, 6);
    recognizer.button(true, true, AT, false, 100);
    recognizer.button(true, false, PhysPoint { x: 100, y: 100 }, false, 200);
    recognizer.button(true, true, PhysPoint { x: -12, y: 22 }, false, 600);
    let up = PhysPoint { x: 500, y: 500 };
    let out = recognizer.button(true, false, up, false, 900);
    assert_eq!(out.len(), 2);
    assert_click(&out[0], up, true, false, 900);
    assert_double(&out[1], up, false, 900);
}

#[test]
fn outside_time_or_rectangle_starts_a_new_pair() {
    for (time, dx, dy) in [
        (501, 0, 0),
        (100, -3, 0),
        (100, 2, 0),
        (100, 0, -4),
        (100, 0, 3),
    ] {
        let mut recognizer = ClickRecognizer::new(500, 4, 6);
        click(&mut recognizer, AT, 0);
        let next = PhysPoint {
            x: AT.x + dx,
            y: AT.y + dy,
        };
        assert_eq!(click(&mut recognizer, next, time).len(), 1);
        assert_eq!(click(&mut recognizer, next, time + 10).len(), 2);
    }
}

#[test]
fn third_click_starts_a_new_pair() {
    let mut recognizer = ClickRecognizer::new(500, 4, 6);
    for (time, count) in [(0, 1), (10, 2), (20, 1), (30, 2)] {
        assert_eq!(click(&mut recognizer, AT, time).len(), count);
    }
}

#[test]
fn mixed_buttons_keep_independent_presses_and_never_pair_non_left() {
    let mut recognizer = ClickRecognizer::new(500, 4, 6);
    recognizer.button(true, true, AT, false, 0);
    recognizer.button(false, true, AT, false, 1);
    let out = recognizer.button(true, false, AT, false, 2);
    assert_eq!(out.len(), 1);
    assert_click(&out[0], AT, true, false, 2);
    let out = recognizer.button(false, false, AT, false, 3);
    assert_eq!(out.len(), 1);
    assert_click(&out[0], AT, false, false, 3);
    recognizer.button(false, true, AT, false, 4);
    assert_eq!(recognizer.button(false, false, AT, false, 5).len(), 1);
    assert_eq!(click(&mut recognizer, AT, 6).len(), 2);
}

#[test]
fn injected_edges_taint_their_click_and_either_click_taints_the_pair() {
    for edge in 0..4 {
        let mut recognizer = ClickRecognizer::new(500, 4, 6);
        recognizer.button(true, true, AT, edge == 0, 0);
        let first = recognizer.button(true, false, AT, edge == 1, 1);
        assert_click(&first[0], AT, true, edge < 2, 1);
        recognizer.button(true, true, AT, edge == 2, 10);
        let second = recognizer.button(true, false, AT, edge == 3, 11);
        assert_eq!(second.len(), 2);
        assert_click(&second[0], AT, true, edge >= 2, 11);
        assert_double(&second[1], AT, true, 11);
    }
}

#[test]
fn unmatched_ups_and_duplicate_downs_do_not_manufacture_clicks() {
    let mut recognizer = ClickRecognizer::new(500, 4, 6);
    assert!(recognizer.button(true, false, AT, false, 0).is_empty());
    recognizer.button(true, true, AT, false, 10);
    assert!(recognizer.button(false, false, AT, false, 11).is_empty());
    assert!(
        recognizer
            .button(true, true, PhysPoint { x: 99, y: 99 }, true, 12)
            .is_empty()
    );
    let out = recognizer.button(true, false, AT, false, 13);
    assert_eq!(out.len(), 1);
    assert_click(&out[0], AT, true, true, 13);
    assert!(recognizer.button(true, false, AT, false, 14).is_empty());
    let out = click(&mut recognizer, AT, 510);
    assert_eq!(out.len(), 2);
    assert_double(&out[1], AT, true, 511);
}

#[test]
fn backward_timestamps_are_clamped_for_presses_and_output() {
    let mut recognizer = ClickRecognizer::new(500, 4, 6);
    recognizer.button(true, true, AT, false, 1000);
    let first = recognizer.button(true, false, AT, false, 900);
    assert_click(&first[0], AT, true, false, 1000);
    recognizer.button(true, true, AT, false, 800);
    let second = recognizer.button(true, false, AT, false, 700);
    assert_eq!(second.len(), 2);
    assert_double(&second[1], AT, false, 1000);
    assert_eq!(click(&mut recognizer, AT, 1100).len(), 1);
    assert_eq!(click(&mut recognizer, AT, 1600).len(), 2);
}

#[test]
fn extreme_coordinates_and_invalid_dimensions_do_not_pair() {
    let low = PhysPoint {
        x: i32::MIN,
        y: i32::MIN,
    };
    let high = PhysPoint {
        x: i32::MAX,
        y: i32::MAX,
    };
    let mut recognizer = ClickRecognizer::new(u32::MAX, i32::MAX, i32::MAX);
    click(&mut recognizer, low, 0);
    assert_eq!(click(&mut recognizer, high, 10).len(), 1);
    for (cx, cy) in [(0, 6), (4, 0), (-4, 6), (4, -6)] {
        let mut recognizer = ClickRecognizer::new(500, cx, cy);
        click(&mut recognizer, AT, 0);
        assert_eq!(click(&mut recognizer, AT, 10).len(), 1);
    }
}
