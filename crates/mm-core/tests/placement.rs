use mm_core::geom::{PhysPoint, PhysRect};
use mm_core::placement::{Placement, Side, place_mate};
use proptest::prelude::*;

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PhysRect {
    PhysRect::new(left, top, right, bottom).unwrap()
}

fn inside(inner: PhysRect, outer: PhysRect) -> bool {
    inner.left >= outer.left
        && inner.top >= outer.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
}

fn clear(mate: PhysRect, target: PhysRect, gap: i32) -> bool {
    let gap = i64::from(gap.max(0));
    i64::from(mate.right) <= i64::from(target.left) - gap
        || i64::from(mate.left) >= i64::from(target.right) + gap
        || i64::from(mate.bottom) <= i64::from(target.top) - gap
        || i64::from(mate.top) >= i64::from(target.bottom) + gap
}

#[test]
fn preferred_sides_and_facing_beam_edges() {
    let work = rect(0, 0, 300, 300);
    for (target, side, mate, beam_from) in [
        (
            rect(100, 100, 150, 150),
            Side::Right,
            rect(160, 105, 220, 145),
            PhysPoint { x: 160, y: 125 },
        ),
        (
            rect(240, 100, 290, 150),
            Side::Left,
            rect(170, 105, 230, 145),
            PhysPoint { x: 230, y: 125 },
        ),
        (
            rect(20, 100, 280, 150),
            Side::Below,
            rect(120, 160, 180, 200),
            PhysPoint { x: 150, y: 160 },
        ),
        (
            rect(20, 240, 280, 290),
            Side::Above,
            rect(120, 190, 180, 230),
            PhysPoint { x: 150, y: 230 },
        ),
    ] {
        let placed = place_mate(target, work, 60, 40, 10).unwrap();
        assert_eq!(
            placed,
            Placement {
                mate,
                beam_from,
                side,
            }
        );
        assert!(inside(placed.mate, work));
        assert!(clear(placed.mate, target, 10));
    }
}

#[test]
fn free_axis_is_clamped_at_both_work_area_edges() {
    let work = rect(0, 0, 300, 300);
    for (target, expected) in [
        (rect(100, 0, 150, 10), rect(160, 0, 220, 40)),
        (rect(100, 290, 150, 300), rect(160, 260, 220, 300)),
        (rect(0, 100, 300, 150), rect(120, 160, 180, 200)),
    ] {
        let placed = place_mate(target, work, 60, 40, 10).unwrap();
        assert_eq!(placed.mate, expected);
        assert!(clear(placed.mate, target, 10));
    }
    // Both horizontal candidates fail; below must clamp its free x axis.
    let placed = place_mate(rect(-100, 100, 50, 150), rect(0, 0, 100, 300), 60, 40, 10).unwrap();
    assert_eq!(placed.side, Side::Below);
    assert_eq!(placed.mate, rect(0, 160, 60, 200));
}

#[test]
fn clamping_into_clearance_rejects_the_preferred_side() {
    let target = rect(220, 100, 245, 150);
    let placed = place_mate(target, rect(0, 0, 300, 300), 60, 40, 10).unwrap();
    assert_eq!(placed.side, Side::Left);
    assert_eq!(placed.mate, rect(150, 105, 210, 145));
    assert!(clear(placed.mate, target, 10));
}

#[test]
fn huge_target_uses_the_farthest_corner_and_nearest_beam_corner() {
    let work = rect(0, 0, 100, 100);
    for (target, mate, beam_from) in [
        (
            rect(0, 0, 70, 70),
            rect(60, 60, 100, 100),
            PhysPoint { x: 60, y: 60 },
        ),
        (
            rect(30, 0, 100, 70),
            rect(0, 60, 40, 100),
            PhysPoint { x: 40, y: 60 },
        ),
        (
            rect(0, 30, 70, 100),
            rect(60, 0, 100, 40),
            PhysPoint { x: 60, y: 40 },
        ),
        (
            rect(30, 30, 100, 100),
            rect(0, 0, 40, 40),
            PhysPoint { x: 40, y: 40 },
        ),
        (work, rect(0, 0, 40, 40), PhysPoint { x: 40, y: 40 }),
    ] {
        let placed = place_mate(target, work, 40, 40, 5).unwrap();
        assert_eq!(
            placed,
            Placement {
                mate,
                beam_from,
                side: Side::Corner,
            }
        );
        assert!(inside(placed.mate, work));
    }
}

#[test]
fn negative_monitor_coordinates_and_exact_clearance() {
    let target = rect(-200, -90, -150, -40);
    let placed = place_mate(target, rect(-300, -200, -100, 0), 40, 40, 10).unwrap();
    assert_eq!(placed.side, Side::Right);
    assert_eq!(placed.mate, rect(-140, -85, -100, -45));
    assert_eq!(placed.beam_from, PhysPoint { x: -140, y: -65 });
    assert!(clear(placed.mate, target, 10));
}

#[test]
fn fit_failure_exact_fit_and_nonpositive_inputs() {
    let work = rect(0, 0, 100, 100);
    for (width, height) in [(101, 40), (40, 101), (0, 40), (-1, 40), (40, 0), (40, -1)] {
        assert_eq!(place_mate(work, work, width, height, 10), None);
    }
    let exact = place_mate(work, work, 100, 100, 10).unwrap();
    assert_eq!(exact.mate, work);
    assert_eq!(exact.side, Side::Corner);
    let target = rect(40, 40, 60, 60);
    assert_eq!(
        place_mate(target, work, 20, 20, -10),
        place_mate(target, work, 20, 20, 0)
    );
}

#[test]
fn extreme_coordinates_and_gap_do_not_overflow() {
    let work = rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
    for target in [
        work,
        rect(i32::MIN, i32::MIN, -1, -1),
        rect(0, 0, i32::MAX, i32::MAX),
    ] {
        let placed = place_mate(target, work, i32::MAX, i32::MAX, i32::MAX).unwrap();
        assert!(inside(placed.mate, work));
        if placed.side != Side::Corner {
            assert!(clear(placed.mate, target, i32::MAX));
        }
    }
}

proptest! {
    #[test]
    fn placement_is_inside_clear_and_deterministic(
        work_x in -100_000i32..100_000,
        work_y in -100_000i32..100_000,
        work_width in 1i32..10_000,
        work_height in 1i32..10_000,
        target_x in -10_000i32..10_000,
        target_y in -10_000i32..10_000,
        target_width in 1i32..20_000,
        target_height in 1i32..20_000,
        mate_width in 1i32..12_000,
        mate_height in 1i32..12_000,
        gap in 0i32..1_000,
    ) {
        let work = rect(work_x, work_y, work_x + work_width, work_y + work_height);
        let target = rect(
            work_x + target_x,
            work_y + target_y,
            work_x + target_x + target_width,
            work_y + target_y + target_height,
        );
        let result = place_mate(target, work, mate_width, mate_height, gap);
        prop_assert_eq!(result, place_mate(target, work, mate_width, mate_height, gap));
        prop_assert_eq!(result.is_some(), mate_width <= work_width && mate_height <= work_height);
        if let Some(placed) = result {
            prop_assert!(inside(placed.mate, work));
            prop_assert_eq!(placed.mate.right - placed.mate.left, mate_width);
            prop_assert_eq!(placed.mate.bottom - placed.mate.top, mate_height);
            if placed.side != Side::Corner {
                prop_assert!(clear(placed.mate, target, gap));
            }
        }
    }
}
