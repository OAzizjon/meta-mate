use mm_core::flight::{MAX_ARC_PX, MAX_DURATION_MS, MIN_DURATION_MS, flight_path};
use mm_core::geom::PhysRect;
use proptest::prelude::*;

fn rect(left: i32, top: i32, width: i32, height: i32) -> PhysRect {
    PhysRect::new(left, top, left + width, top + height).unwrap()
}

#[test]
fn endpoints_and_stationary_flight() {
    let from = rect(100, 500, 220, 220);
    for (duration_ms, frame_ms) in [(0, 0), (150, 16), (300, 15), (900, 1000)] {
        assert_eq!(flight_path(from, from, duration_ms, frame_ms), vec![from]);
    }

    let to = rect(700, 300, 240, 180);
    let path = flight_path(from, to, 300, 15);
    assert_eq!(path.len(), 20);
    assert_ne!(path.first(), Some(&from));
    assert_eq!(path.last(), Some(&to));
    for frame in &path {
        assert_eq!(frame.right - frame.left, 240);
        assert_eq!(frame.bottom - frame.top, 180);
    }
}

#[test]
fn frame_count_and_duration_clamping() {
    let from = rect(0, 400, 220, 220);
    let to = rect(600, 400, 220, 220);

    let min_path = flight_path(from, to, MIN_DURATION_MS, 15);
    assert_eq!(min_path.len(), 10);
    assert_eq!(flight_path(from, to, 0, 15), min_path);
    assert_eq!(flight_path(from, to, 50, 15), min_path);
    assert_eq!(flight_path(from, to, 149, 15), min_path);

    let max_path = flight_path(from, to, MAX_DURATION_MS, 15);
    assert_eq!(max_path.len(), 60);
    assert_eq!(flight_path(from, to, 901, 15), max_path);
    assert_eq!(flight_path(from, to, 10_000, 15), max_path);

    assert_eq!(flight_path(from, to, 240, 16).len(), 15);
    assert_eq!(flight_path(from, to, 300, 1000), vec![to]);
    assert_eq!(flight_path(from, to, 300, 0), vec![to]);
}

#[test]
fn midpoint_arc_lift_and_upper_bound() {
    // Short horizontal flight: distance = 120, midpoint lifted by 120 / 4 = 30 px.
    let from = rect(100, 500, 220, 220);
    let short_to = rect(220, 500, 220, 220);
    let short_path = flight_path(from, short_to, 300, 30);
    assert_eq!(short_path.len(), 10);
    let short_mid = short_path[4];
    assert_eq!(short_mid.left, 160);
    assert_eq!(short_mid.top, 470);
    assert!(short_path.iter().all(|r| r.top >= 470));

    // Long horizontal flight: distance = 600, midpoint lift capped at 80 px.
    let long_to = rect(700, 500, 220, 220);
    let long_path = flight_path(from, long_to, 300, 30);
    let long_mid = long_path[4];
    assert_eq!(long_mid.left, 400);
    assert_eq!(long_mid.top, 500 - MAX_ARC_PX);
    assert!(long_path.iter().all(|r| r.top >= 500 - MAX_ARC_PX));

    // Upward diagonal flight: straight-line midpoint top is 400, lifted by 80 to 320;
    // never above min(500, 300) - 80 = 220.
    let diag_to = rect(500, 300, 220, 220);
    let diag_path = flight_path(from, diag_to, 300, 30);
    assert_eq!(diag_path[4].left, 300);
    assert_eq!(diag_path[4].top, 320);
    assert!(diag_path.iter().all(|r| r.top >= 300 - MAX_ARC_PX));
}

#[test]
fn monotonic_progress_and_cubic_ease_in_out() {
    let from = rect(-400, 300, 220, 220);
    let to = rect(600, 300, 220, 220);
    let path = flight_path(from, to, 300, 30);

    let mut prev_left = from.left;
    for frame in &path {
        assert!(frame.left >= prev_left, "{} < {}", frame.left, prev_left);
        assert!(frame.left <= to.left);
        prev_left = frame.left;
    }

    // Ease-in-out starts and finishes with smaller horizontal steps than at the midpoint.
    let first_step = path[0].left - from.left;
    let mid_step = path[5].left - path[4].left;
    let last_step = path[9].left - path[8].left;
    assert!(mid_step > first_step);
    assert!(mid_step > last_step);

    // Pure vertical flights (both upward and downward) progress monotonically in Y.
    for target_top in [-100, 700] {
        let v_to = rect(-400, target_top, 220, 220);
        let v_path = flight_path(from, v_to, 300, 25);
        let mut prev_dist = (v_to.top - from.top).abs();
        for frame in &v_path {
            let dist = (v_to.top - frame.top).abs();
            assert!(dist <= prev_dist, "{dist} > {prev_dist}");
            prev_dist = dist;
        }
    }
}

proptest! {
    #[test]
    fn flight_invariants_hold(
        from_left in -10_000i32..=10_000,
        from_top in -10_000i32..=10_000,
        from_w in 1i32..=500,
        from_h in 1i32..=500,
        to_left in -10_000i32..=10_000,
        to_top in -10_000i32..=10_000,
        to_w in 1i32..=500,
        to_h in 1i32..=500,
        duration_ms in 0u32..=2_000,
        frame_ms in 1u32..=300,
    ) {
        let from = rect(from_left, from_top, from_w, from_h);
        let to = rect(to_left, to_top, to_w, to_h);
        let path = flight_path(from, to, duration_ms, frame_ms);

        prop_assert!(!path.is_empty());
        prop_assert_eq!(path.last(), Some(&to));

        if from == to {
            prop_assert_eq!(path, vec![to]);
        } else {
            let expected_len = (duration_ms.clamp(MIN_DURATION_MS, MAX_DURATION_MS) / frame_ms)
                .max(1) as usize;
            prop_assert_eq!(path.len(), expected_len);
            prop_assert_ne!(path.first(), Some(&from));

            let min_top = from.top.min(to.top) - MAX_ARC_PX;
            let mut prev_x_dist = (i64::from(to.left) - i64::from(from.left)).abs();
            let mut prev_y_dist = (i64::from(to.top) - i64::from(from.top)).abs();

            for frame in &path {
                prop_assert_eq!(frame.right - frame.left, to_w);
                prop_assert_eq!(frame.bottom - frame.top, to_h);
                prop_assert!(frame.top >= min_top);

                let x_dist = (i64::from(to.left) - i64::from(frame.left)).abs();
                prop_assert!(x_dist <= prev_x_dist);
                prev_x_dist = x_dist;

                if from.left == to.left {
                    let y_dist = (i64::from(to.top) - i64::from(frame.top)).abs();
                    prop_assert!(y_dist <= prev_y_dist);
                    prev_y_dist = y_dist;
                }
            }
        }
    }
}
