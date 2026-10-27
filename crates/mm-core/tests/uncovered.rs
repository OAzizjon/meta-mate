use mm_core::geom::{PhysPoint, PhysRect, uncovered};
use proptest::prelude::*;

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PhysRect {
    PhysRect {
        left,
        top,
        right,
        bottom,
    }
}

fn valid(rect: PhysRect) -> bool {
    rect.left < rect.right && rect.top < rect.bottom
}

fn intersects(a: PhysRect, b: PhysRect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

fn area(rect: PhysRect) -> i64 {
    (i64::from(rect.right) - i64::from(rect.left)) * (i64::from(rect.bottom) - i64::from(rect.top))
}

#[test]
fn no_covers_leaves_region() {
    let region = rect(-10, -20, 30, 40);
    assert_eq!(uncovered(region, &[]), vec![region]);
}

#[test]
fn equal_cover_leaves_nothing() {
    let region = rect(0, 0, 10, 10);
    assert!(uncovered(region, &[region]).is_empty());
}

#[test]
fn middle_cover_leaves_four_pieces() {
    let result = uncovered(rect(0, 0, 10, 10), &[rect(3, 3, 7, 7)]);
    assert_eq!(
        result,
        vec![
            rect(0, 0, 10, 3),
            rect(0, 3, 3, 7),
            rect(7, 3, 10, 7),
            rect(0, 7, 10, 10),
        ]
    );
    assert_eq!(result.iter().copied().map(area).sum::<i64>(), 84);
}

#[test]
fn overlapping_covers_are_subtracted_once() {
    let covers = [rect(0, 0, 6, 6), rect(4, 4, 10, 10)];
    let result = uncovered(rect(0, 0, 10, 10), &covers);
    assert_eq!(result, vec![rect(6, 0, 10, 4), rect(0, 6, 4, 10)]);
    assert_eq!(result.iter().copied().map(area).sum::<i64>(), 32);
    assert_eq!(
        result,
        uncovered(rect(0, 0, 10, 10), &[covers[1], covers[0], covers[0]])
    );
}

#[test]
fn covers_are_clipped_to_region() {
    let region = rect(0, 0, 10, 10);
    assert_eq!(
        uncovered(region, &[rect(-5, -5, 4, 20), rect(10, 0, 20, 10)]),
        vec![rect(4, 0, 10, 10)],
    );
    assert!(uncovered(region, &[rect(-5, -5, 20, 20)]).is_empty());
}

#[test]
fn empty_and_inverted_covers_are_ignored() {
    let region = rect(0, 0, 10, 10);
    let covers = [
        rect(3, 2, 3, 8),
        rect(2, 3, 8, 3),
        rect(8, 2, 2, 8),
        rect(2, 8, 8, 2),
    ];
    assert_eq!(uncovered(region, &covers), vec![region]);
}

#[test]
fn empty_and_inverted_regions_leave_nothing() {
    for region in [
        rect(0, 0, 0, 10),
        rect(0, 0, 10, 0),
        rect(10, 0, 0, 10),
        rect(0, 10, 10, 0),
    ] {
        assert!(uncovered(region, &[]).is_empty());
    }
}

#[test]
fn extreme_edges_need_no_coordinate_arithmetic() {
    let region = rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
    assert_eq!(
        uncovered(region, &[rect(i32::MIN, i32::MIN, 0, i32::MAX)]),
        vec![rect(0, i32::MIN, i32::MAX, i32::MAX)],
    );
}

proptest! {
    #[test]
    fn uncovered_partitions_region(
        x1 in 0i32..64, x2 in 0i32..64,
        y1 in 0i32..64, y2 in 0i32..64,
        raw_covers in prop::collection::vec((-8i32..73, -8i32..73, -8i32..73, -8i32..73), 0..=8),
    ) {
        let region = rect(x1.min(x2), y1.min(y2), x1.max(x2) + 1, y1.max(y2) + 1);
        let covers: Vec<_> = raw_covers.into_iter().map(|(l, t, r, b)| rect(l, t, r, b)).collect();
        let result = uncovered(region, &covers);
        for (i, output) in result.iter().enumerate() {
            prop_assert!(valid(*output));
            prop_assert!(region.left <= output.left && output.right <= region.right);
            prop_assert!(region.top <= output.top && output.bottom <= region.bottom);
            for other in &result[i + 1..] {
                prop_assert!(!intersects(*output, *other));
            }
            for cover in covers.iter().copied().filter(|cover| valid(*cover)) {
                prop_assert!(!intersects(*output, cover));
            }
        }
        for pair in result.windows(2) {
            prop_assert!((pair[0].top, pair[0].left) < (pair[1].top, pair[1].left));
            prop_assert!(pair[0].top != pair[1].top || pair[0].right < pair[1].left);
        }
        // Independent union-area oracle: count unit pixels in the small integer region.
        let mut covered_area = 0i64;
        for y in region.top..region.bottom {
            for x in region.left..region.right {
                if covers.iter().any(|cover| valid(*cover) && cover.contains(PhysPoint { x, y })) {
                    covered_area += 1;
                }
            }
        }
        let uncovered_area: i64 = result.iter().copied().map(area).sum();
        prop_assert_eq!(uncovered_area + covered_area, area(region));
    }
}
