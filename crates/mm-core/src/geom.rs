//! Distinct coordinate spaces for capture, model grounding and drawing.
//!
//! Integer conversions round to nearest, with ties away from zero. Rectangles
//! have half-open bounds; their right/bottom edges are geometric boundaries.
//!
//! ```compile_fail
//! use mm_core::geom::{PhysPoint, ShotPoint};
//! fn hit_test(_: PhysPoint) {}
//! hit_test(ShotPoint { x: 10.0, y: 20.0 });
//! ```

/// Invalid geometry supplied to a constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeomError {
    EmptyRect,
    InvalidImageSize,
    InvalidScale,
    NonFiniteCoordinate,
}

impl std::fmt::Display for GeomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::EmptyRect => "rectangle must have positive width and height",
            Self::InvalidImageSize => "sent image dimensions must be positive",
            Self::InvalidScale => "monitor scale must be finite and positive",
            Self::NonFiniteCoordinate => "coordinates must be finite",
        })
    }
}

impl std::error::Error for GeomError {}

/// Physical pixels of the Windows virtual screen, including negative positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysPoint {
    pub x: i32,
    pub y: i32,
}

/// Half-open physical virtual-screen rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PhysRect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Result<Self, GeomError> {
        if left >= right || top >= bottom {
            return Err(GeomError::EmptyRect);
        }
        Ok(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    /// Includes left/top and excludes right/bottom, like Windows RECT.
    pub fn contains(self, point: PhysPoint) -> bool {
        self.left <= point.x && point.x < self.right && self.top <= point.y && point.y < self.bottom
    }

    /// Geometric center, rounded to nearest with ties away from zero.
    pub fn center(self) -> PhysPoint {
        PhysPoint {
            x: ((f64::from(self.left) + f64::from(self.right)) / 2.0).round() as i32,
            y: ((f64::from(self.top) + f64::from(self.bottom)) / 2.0).round() as i32,
        }
    }

    /// Euclidean distance to the geometric rectangle, zero inside or on an edge.
    /// An excluded right/bottom boundary still has distance zero, matching
    /// the HIT / NEAR / MISS distance calculation in `tools/mm_fixture.py`.
    pub fn distance_to(self, point: PhysPoint) -> f64 {
        let dx = (f64::from(self.left) - f64::from(point.x))
            .max(0.0)
            .max(f64::from(point.x) - f64::from(self.right));
        let dy = (f64::from(self.top) - f64::from(point.y))
            .max(0.0)
            .max(f64::from(point.y) - f64::from(self.bottom));
        dx.hypot(dy)
    }
}

/// Disjoint parts of `region` outside all covers, ordered top to bottom, left to right.
/// Empty/inverted regions yield no rectangles; empty/inverted covers are ignored.
/// Clipped cover edges form O(n^2) cells, merged horizontally within each row band.
pub fn uncovered(region: PhysRect, covers: &[PhysRect]) -> Vec<PhysRect> {
    if region.left >= region.right || region.top >= region.bottom {
        return Vec::new();
    }
    let covers: Vec<PhysRect> = covers
        .iter()
        .filter_map(|cover| {
            let clipped = PhysRect {
                left: region.left.max(cover.left),
                top: region.top.max(cover.top),
                right: region.right.min(cover.right),
                bottom: region.bottom.min(cover.bottom),
            };
            (clipped.left < clipped.right && clipped.top < clipped.bottom).then_some(clipped)
        })
        .collect();
    let mut xs = vec![region.left, region.right];
    let mut ys = vec![region.top, region.bottom];
    for cover in &covers {
        xs.extend([cover.left, cover.right]);
        ys.extend([cover.top, cover.bottom]);
    }
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();

    let mut result: Vec<PhysRect> = Vec::new();
    for y in ys.windows(2) {
        for x in xs.windows(2) {
            // Every cover boundary is a grid edge, so one corner classifies the cell.
            if covers
                .iter()
                .any(|cover| cover.contains(PhysPoint { x: x[0], y: y[0] }))
            {
                continue;
            }
            if let Some(previous) = result.last_mut()
                && previous.top == y[0]
                && previous.right == x[0]
            {
                previous.right = x[1];
                continue;
            }
            result.push(PhysRect {
                left: x[0],
                top: y[0],
                right: x[1],
                bottom: y[1],
            });
        }
    }
    result
}

macro_rules! float_geometry {
    ($point:ident, $rect:ident, $space:literal) => {
        #[doc = $space]
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct $point {
            pub x: f64,
            pub y: f64,
        }

        impl $point {
            pub fn new(x: f64, y: f64) -> Result<Self, GeomError> {
                if !x.is_finite() || !y.is_finite() {
                    return Err(GeomError::NonFiniteCoordinate);
                }
                Ok(Self { x, y })
            }
        }

        #[doc = $space]
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct $rect {
            pub left: f64,
            pub top: f64,
            pub right: f64,
            pub bottom: f64,
        }

        impl $rect {
            pub fn new(left: f64, top: f64, right: f64, bottom: f64) -> Result<Self, GeomError> {
                if ![left, top, right, bottom].iter().all(|v| v.is_finite()) {
                    return Err(GeomError::NonFiniteCoordinate);
                }
                if left >= right || top >= bottom {
                    return Err(GeomError::EmptyRect);
                }
                Ok(Self {
                    left,
                    top,
                    right,
                    bottom,
                })
            }
        }
    };
}

float_geometry!(
    ShotPoint,
    ShotRect,
    "Pixels of the downscaled screenshot sent to the model."
);
float_geometry!(
    CssPoint,
    CssRect,
    "CSS pixels relative to one monitor's WebView origin."
);

/// Mapping belonging to one captured frame; independent of monitor DPI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameTransform {
    region: PhysRect,
    sent_width: u32,
    sent_height: u32,
}

impl FrameTransform {
    pub fn new(region: PhysRect, sent_width: u32, sent_height: u32) -> Result<Self, GeomError> {
        // Revalidate because public rectangle fields allow struct literals.
        PhysRect::new(region.left, region.top, region.right, region.bottom)?;
        if sent_width == 0 || sent_height == 0 {
            return Err(GeomError::InvalidImageSize);
        }
        Ok(Self {
            region,
            sent_width,
            sent_height,
        })
    }

    /// The captured region in physical pixels.
    pub fn region(self) -> PhysRect {
        self.region
    }

    /// The size of the image sent to the model: its answers are pixels of this image.
    pub fn sent_size(self) -> (u32, u32) {
        (self.sent_width, self.sent_height)
    }

    /// Like `shot_to_phys`, but a point outside the sent image (or not finite) is `None`
    /// instead of being clamped to the edge: a model's point off the screenshot is a
    /// guess, never a target (invariant 5).
    pub fn shot_to_phys_checked(self, point: ShotPoint) -> Option<PhysPoint> {
        let inside = |v: f64, size: u32| v.is_finite() && v >= 0.0 && v < f64::from(size);
        (inside(point.x, self.sent_width) && inside(point.y, self.sent_height))
            .then(|| self.shot_to_phys(point))
    }

    fn width(self) -> f64 {
        f64::from(self.region.right) - f64::from(self.region.left)
    }

    fn height(self) -> f64 {
        f64::from(self.region.bottom) - f64::from(self.region.top)
    }

    /// Clamp to image boundaries `[0, width]` and `[0, height]` before mapping.
    /// Outer boundaries map to the capture's right/bottom edges, excluded by
    /// `contains`. Round to nearest, ties away from zero. For unchecked struct
    /// literals, NaN maps to the corresponding origin and infinities to edges.
    pub fn shot_to_phys(self, point: ShotPoint) -> PhysPoint {
        let x = point.x.max(0.0).min(f64::from(self.sent_width));
        let y = point.y.max(0.0).min(f64::from(self.sent_height));
        PhysPoint {
            x: (f64::from(self.region.left) + x / f64::from(self.sent_width) * self.width()).round()
                as i32,
            y: (f64::from(self.region.top) + y / f64::from(self.sent_height) * self.height())
                .round() as i32,
        }
    }

    /// No clamping: off-capture element rectangles retain their positions.
    pub fn phys_to_shot(self, point: PhysPoint) -> ShotPoint {
        ShotPoint {
            x: (f64::from(point.x) - f64::from(self.region.left)) / self.width()
                * f64::from(self.sent_width),
            y: (f64::from(point.y) - f64::from(self.region.top)) / self.height()
                * f64::from(self.sent_height),
        }
    }

    /// Map both corners with `shot_to_phys`; clipping/rounding may collapse a rect.
    pub fn shot_rect_to_phys(self, rect: ShotRect) -> PhysRect {
        let a = self.shot_to_phys(ShotPoint {
            x: rect.left,
            y: rect.top,
        });
        let b = self.shot_to_phys(ShotPoint {
            x: rect.right,
            y: rect.bottom,
        });
        PhysRect {
            left: a.x,
            top: a.y,
            right: b.x,
            bottom: b.y,
        }
    }

    pub fn phys_rect_to_shot(self, rect: PhysRect) -> ShotRect {
        let a = self.phys_to_shot(PhysPoint {
            x: rect.left,
            y: rect.top,
        });
        let b = self.phys_to_shot(PhysPoint {
            x: rect.right,
            y: rect.bottom,
        });
        ShotRect {
            left: a.x,
            top: a.y,
            right: b.x,
            bottom: b.y,
        }
    }
}

/// Drawing transform for a monitor's WebView at 100% WebView zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorTransform {
    origin: PhysPoint,
    scale: f64,
}

impl MonitorTransform {
    pub fn new(origin: PhysPoint, scale: f64) -> Result<Self, GeomError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(GeomError::InvalidScale);
        }
        Ok(Self { origin, scale })
    }

    pub fn phys_to_css(self, point: PhysPoint) -> CssPoint {
        CssPoint {
            x: (f64::from(point.x) - f64::from(self.origin.x)) / self.scale,
            y: (f64::from(point.y) - f64::from(self.origin.y)) / self.scale,
        }
    }

    /// Round to nearest, ties away from zero. Values outside i32 saturate;
    /// NaN supplied through unchecked public fields converts to zero.
    pub fn css_to_phys(self, point: CssPoint) -> PhysPoint {
        PhysPoint {
            x: (f64::from(self.origin.x) + point.x * self.scale).round() as i32,
            y: (f64::from(self.origin.y) + point.y * self.scale).round() as i32,
        }
    }

    pub fn phys_rect_to_css(self, rect: PhysRect) -> CssRect {
        let a = self.phys_to_css(PhysPoint {
            x: rect.left,
            y: rect.top,
        });
        let b = self.phys_to_css(PhysPoint {
            x: rect.right,
            y: rect.bottom,
        });
        CssRect {
            left: a.x,
            top: a.y,
            right: b.x,
            bottom: b.y,
        }
    }

    /// Map both corners with `css_to_phys`; rounding may collapse a rectangle.
    pub fn css_rect_to_phys(self, rect: CssRect) -> PhysRect {
        let a = self.css_to_phys(CssPoint {
            x: rect.left,
            y: rect.top,
        });
        let b = self.css_to_phys(CssPoint {
            x: rect.right,
            y: rect.bottom,
        });
        PhysRect {
            left: a.x,
            top: a.y,
            right: b.x,
            bottom: b.y,
        }
    }
}
