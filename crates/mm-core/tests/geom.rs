use mm_core::geom::{
    CssPoint, CssRect, FrameTransform, GeomError, MonitorTransform, PhysPoint, PhysRect, ShotPoint,
    ShotRect,
};

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

#[test]
fn dpi_scaling_and_negative_monitor_origin() {
    for scale in [1.0, 1.25, 1.5, 1.75] {
        let monitor = MonitorTransform::new(PhysPoint { x: -1920, y: 0 }, scale).unwrap();
        let css = CssPoint { x: 320.0, y: 240.0 };
        let physical = monitor.css_to_phys(css);
        assert_eq!(
            physical,
            PhysPoint {
                x: -1920 + (320.0 * scale) as i32,
                y: (240.0 * scale) as i32,
            }
        );
        assert_eq!(monitor.phys_to_css(physical), css);
        for x in [-1920, -1919, -1433, -1, 0, 640] {
            for y in [-200, 0, 1, 351, 1599] {
                let point = PhysPoint { x, y };
                let back = monitor.css_to_phys(monitor.phys_to_css(point));
                assert!((i64::from(back.x) - i64::from(x)).abs() <= 1);
                assert!((i64::from(back.y) - i64::from(y)).abs() <= 1);
            }
        }
    }
}

#[test]
fn downscaled_frame_edges_and_clamping() {
    let region = PhysRect::new(-1920, -200, 640, 1400).unwrap();
    let frame = FrameTransform::new(region, 1280, 800).unwrap();
    for (shot, phys) in [
        ((0.0, 0.0), (-1920, -200)),
        ((1280.0, 0.0), (640, -200)),
        ((0.0, 800.0), (-1920, 1400)),
        ((1280.0, 800.0), (640, 1400)),
        ((640.0, 400.0), (-640, 600)),
        ((-1.0, 900.0), (-1920, 1400)),
        ((1400.0, -1.0), (640, -200)),
        ((1279.0, 799.0), (638, 1398)),
    ] {
        assert_eq!(
            frame.shot_to_phys(ShotPoint {
                x: shot.0,
                y: shot.1
            }),
            PhysPoint {
                x: phys.0,
                y: phys.1
            }
        );
    }
    for x in [-1920, -1919, -1201, -1, 639, 640] {
        for y in [-200, -199, 1, 777, 1399, 1400] {
            let point = PhysPoint { x, y };
            assert_eq!(frame.shot_to_phys(frame.phys_to_shot(point)), point);
        }
    }
    let outside = frame.phys_to_shot(PhysPoint { x: -1922, y: 1402 });
    assert_eq!(outside, ShotPoint { x: -1.0, y: 801.0 });
}

#[test]
fn independent_axes_and_nearest_rounding() {
    let frame = FrameTransform::new(PhysRect::new(-10, -10, 11, 20).unwrap(), 10, 10).unwrap();
    assert_eq!(
        frame.shot_to_phys(ShotPoint { x: 5.0, y: 5.0 }),
        PhysPoint { x: 1, y: 5 }
    );
    let frame = FrameTransform::new(PhysRect::new(-2, -2, 2, 2).unwrap(), 8, 8).unwrap();
    assert_eq!(
        frame.shot_to_phys(ShotPoint { x: 1.0, y: 7.0 }),
        PhysPoint { x: -2, y: 2 }
    );
    let monitor = MonitorTransform::new(PhysPoint { x: 0, y: 0 }, 1.0).unwrap();
    assert_eq!(
        monitor.css_to_phys(CssPoint { x: -1.5, y: 1.5 }),
        PhysPoint { x: -2, y: 2 }
    );
}

#[test]
fn rects_map_corner_by_corner() {
    let frame =
        FrameTransform::new(PhysRect::new(-1920, 100, 640, 1700).unwrap(), 1280, 800).unwrap();
    let shot = ShotRect::new(10.25, 20.25, 300.75, 400.75).unwrap();
    let physical = frame.shot_rect_to_phys(shot);
    assert_eq!(physical, PhysRect::new(-1900, 141, -1319, 902).unwrap());
    assert_eq!(
        frame.shot_to_phys(ShotPoint {
            x: shot.left,
            y: shot.top
        }),
        PhysPoint {
            x: physical.left,
            y: physical.top
        }
    );
    assert_eq!(
        frame.shot_to_phys(ShotPoint {
            x: shot.right,
            y: shot.bottom
        }),
        PhysPoint {
            x: physical.right,
            y: physical.bottom
        }
    );
    let back = frame.phys_rect_to_shot(physical);
    assert!((back.left - shot.left).abs() <= 0.5);
    assert!((back.top - shot.top).abs() <= 0.5);
    assert!((back.right - shot.right).abs() <= 0.5);
    assert!((back.bottom - shot.bottom).abs() <= 0.5);
    assert_eq!(
        frame.shot_rect_to_phys(ShotRect::new(-100.0, -100.0, 1400.0, 900.0).unwrap()),
        PhysRect::new(-1920, 100, 640, 1700).unwrap()
    );
    for scale in [1.0, 1.25, 1.5, 1.75] {
        let monitor = MonitorTransform::new(PhysPoint { x: -1920, y: 0 }, scale).unwrap();
        let css = monitor.phys_rect_to_css(physical);
        close(css.left, 20.0 / scale);
        close(css.top, 141.0 / scale);
        close(css.right, 601.0 / scale);
        close(css.bottom, 902.0 / scale);
        assert_eq!(monitor.css_rect_to_phys(css), physical);
        let a = monitor.css_to_phys(CssPoint {
            x: css.left,
            y: css.top,
        });
        let b = monitor.css_to_phys(CssPoint {
            x: css.right,
            y: css.bottom,
        });
        assert_eq!(
            (physical.left, physical.top, physical.right, physical.bottom),
            (a.x, a.y, b.x, b.y)
        );
    }
}

#[test]
fn hit_near_miss_and_extreme_coordinates() {
    let rect = PhysRect::new(-100, -100, 100, 100).unwrap();
    assert!(rect.contains(PhysPoint { x: -100, y: -100 }));
    assert!(!rect.contains(PhysPoint { x: 100, y: 0 }));
    assert!(!rect.contains(PhysPoint { x: 0, y: 100 }));
    assert_eq!(rect.center(), PhysPoint { x: 0, y: 0 });
    for (point, distance) in [
        (PhysPoint { x: 0, y: 0 }, 0.0),
        (PhysPoint { x: 100, y: 100 }, 0.0),
        (PhysPoint { x: 140, y: 0 }, 40.0),
        (PhysPoint { x: -124, y: -132 }, 40.0),
        (PhysPoint { x: 141, y: 0 }, 41.0),
    ] {
        close(rect.distance_to(point), distance);
    }
    let huge = PhysRect::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX).unwrap();
    assert_eq!(huge.center(), PhysPoint { x: -1, y: -1 });
    let frame = FrameTransform::new(huge, 1, 1).unwrap();
    assert_eq!(
        frame.shot_to_phys(ShotPoint { x: 1.0, y: 1.0 }),
        PhysPoint {
            x: i32::MAX,
            y: i32::MAX
        }
    );
    assert!(
        rect.distance_to(PhysPoint {
            x: i32::MIN,
            y: i32::MAX
        })
        .is_finite()
    );
}

#[test]
fn constructors_reject_invalid_geometry() {
    for (left, top, right, bottom) in [(0, 0, 0, 1), (0, 0, 1, 0), (2, 0, 1, 1)] {
        assert_eq!(
            PhysRect::new(left, top, right, bottom),
            Err(GeomError::EmptyRect)
        );
        assert!(ShotRect::new(left as f64, top as f64, right as f64, bottom as f64).is_err());
        assert!(CssRect::new(left as f64, top as f64, right as f64, bottom as f64).is_err());
    }
    let region = PhysRect::new(0, 0, 100, 100).unwrap();
    assert_eq!(
        FrameTransform::new(region, 0, 1),
        Err(GeomError::InvalidImageSize)
    );
    assert_eq!(
        FrameTransform::new(region, 1, 0),
        Err(GeomError::InvalidImageSize)
    );
    let invalid = PhysRect {
        left: 1,
        top: 0,
        right: 0,
        bottom: 1,
    };
    assert_eq!(
        FrameTransform::new(invalid, 1, 1),
        Err(GeomError::EmptyRect)
    );
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            MonitorTransform::new(PhysPoint { x: 0, y: 0 }, scale),
            Err(GeomError::InvalidScale)
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(ShotPoint::new(value, 0.0).is_err());
        assert!(CssPoint::new(0.0, value).is_err());
        assert!(ShotRect::new(0.0, 0.0, value, 1.0).is_err());
        assert!(CssRect::new(0.0, value, 1.0, 1.0).is_err());
    }
}
