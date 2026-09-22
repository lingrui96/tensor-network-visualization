//! Tensor shapes (docs/language.md, section 8.2).

use super::path::{Path, circle, fillet_closed};
use crate::layout::V3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Orb,
    Triangle,
    Diamond,
    Dot,
}

impl Shape {
    pub fn from_word(w: &str) -> Option<Shape> {
        Some(match w {
            "rect" => Shape::Rect,
            "orb" => Shape::Orb,
            "triangle" => Shape::Triangle,
            "diamond" => Shape::Diamond,
            "dot" => Shape::Dot,
            _ => return None,
        })
    }

    /// The default box of the registry, in layout units.
    pub fn default_size(self) -> (f64, f64) {
        match self {
            Shape::Rect => (1.1, 0.75),
            Shape::Orb => (0.8, 0.8),
            Shape::Triangle => (1.0, 0.85),
            Shape::Diamond => (1.05, 0.85),
            Shape::Dot => (0.15, 0.15),
        }
    }

    /// Whether the shape is a circle, whose height follows its width.
    pub fn is_round(self) -> bool {
        matches!(self, Shape::Orb | Shape::Dot)
    }

    /// Whether a box of half-size `a` × `b` centred in a `w` × `h` shape
    /// fits inside it.
    pub fn fits(self, w: f64, h: f64, a: f64, b: f64) -> bool {
        match self {
            Shape::Rect => a <= w / 2.0 && b <= h / 2.0,
            Shape::Orb | Shape::Dot => a.hypot(b) <= w / 2.0,
            // The half-width at height y is (w/2)(1/2 − y/h); the box's top
            // corners are the tightest.
            Shape::Triangle => b < h / 2.0 && a <= (w / 2.0) * (0.5 - b / h),
            Shape::Diamond => 2.0 * a / w + 2.0 * b / h <= 1.0,
        }
    }

    /// The smallest size, at least `(w, h)`, that fits a label box of
    /// half-size `a` × `b`.  Polygons grow uniformly so that they keep their
    /// proportions; a rectangle grows along each axis separately.
    pub fn fit(self, (w, h): (f64, f64), a: f64, b: f64) -> (f64, f64) {
        match self {
            Shape::Rect => (w.max(2.0 * a), h.max(2.0 * b)),
            Shape::Orb | Shape::Dot => {
                let d = w.max(2.0 * a.hypot(b));
                (d, d)
            }
            Shape::Triangle => {
                let s = (4.0 / w * (a + b * w / (2.0 * h))).max(1.0);
                (w * s, h * s)
            }
            Shape::Diamond => {
                let s = (2.0 * a / w + 2.0 * b / h).max(1.0);
                (w * s, h * s)
            }
        }
    }

    /// The sharp corners of a polygonal shape, counter-clockwise, in the
    /// local frame; `None` for round shapes.
    pub fn corners(self, w: f64, h: f64) -> Option<Vec<V3>> {
        let (x, y) = (w / 2.0, h / 2.0);
        Some(match self {
            Shape::Orb | Shape::Dot => return None,
            Shape::Rect => vec![V3::xy(-x, -y), V3::xy(x, -y), V3::xy(x, y), V3::xy(-x, y)],
            Shape::Triangle => vec![V3::xy(-x, -y), V3::xy(x, -y), V3::xy(0.0, y)],
            Shape::Diamond => vec![V3::xy(0.0, -y), V3::xy(x, 0.0), V3::xy(0.0, y), V3::xy(-x, 0.0)],
        })
    }

    /// The outline in the tensor's local frame, and the corner radius
    /// actually used.
    pub fn outline(self, w: f64, h: f64, corner: f64) -> (Path, f64) {
        match self.corners(w, h) {
            None => (circle(V3::ZERO, w / 2.0), 0.0),
            Some(corners) => fillet_closed(&corners, corner.max(0.0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting() {
        for shape in [Shape::Rect, Shape::Orb, Shape::Triangle, Shape::Diamond] {
            let (w, h) = shape.fit(shape.default_size(), 0.6, 0.3);
            assert!(shape.fits(w * 1.0001, h * 1.0001, 0.6, 0.3), "{shape:?}");
            let small = shape.fit(shape.default_size(), 0.01, 0.01);
            assert_eq!(small, shape.default_size());
        }
    }

    #[test]
    fn outlines_have_the_right_extent() {
        let (tri, _) = Shape::Triangle.outline(2.0, 2.0, 0.0);
        let up = tri.ray_distance(V3::ZERO, V3::xy(0.0, 1.0)).unwrap();
        let down = tri.ray_distance(V3::ZERO, V3::xy(0.0, -1.0)).unwrap();
        assert!((up - 1.0).abs() < 1e-9 && (down - 1.0).abs() < 1e-9);
        let (dia, used) = Shape::Diamond.outline(2.0, 2.0, 10.0);
        assert!(used < 10.0);
        let right = dia.ray_distance(V3::ZERO, V3::xy(1.0, 0.0)).unwrap();
        assert!(right < 1.0);
    }
}
