//! The order stage (docs/language.md, section 8.9): the pieces of a
//! figure, sorted into drawing order by (pass, depth, class, z, order).

use std::cmp::Ordering;

use crate::geometry::{ArrowGeom, Geometry, LabelOwner, LineKind};
use crate::layout::V3;
use crate::model::{Network, TensorId};
use crate::registry::{number, word};

/// A piece of the figure, drawn as one unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    TensorShadow(TensorId),
    Tensor(TensorId),
    /// A bond or leg, by its position in `Geometry::lines`.
    Line(usize),
    /// A line's arrowhead, drawn right after the line.
    Arrow(usize),
    /// In 3D, one span of a line (`Geometry::lines[k].spans[i]`).
    Span(usize, usize),
    /// In 3D, a plane.
    Plane(usize),
    /// A label, by its position in `Geometry::labels`.
    Label(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pass {
    Background,
    Scene,
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Bonds,
    Shadows,
    Tensors,
    Front,
}

/// A sort key, compared field by field.  Depth sorts far (larger) first;
/// the other fields ascending.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub pass: Pass,
    /// In 3D, the layer among the planes (language section 11.7); 0 in 2D.
    pub layer: u32,
    pub depth: f64,
    pub class: Class,
    pub z: f64,
    /// Source order; parts of one object differ in the fraction.
    pub order: f64,
}

impl Key {
    fn cmp(&self, other: &Key) -> Ordering {
        self.pass
            .cmp(&other.pass)
            .then(self.layer.cmp(&other.layer))
            .then(other.depth.total_cmp(&self.depth))
            .then(self.class.cmp(&other.class))
            .then(self.z.total_cmp(&other.z))
            .then(self.order.total_cmp(&other.order))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fragment {
    pub key: Key,
    pub part: Part,
}

/// The fragments of a figure in drawing order.  In 2D every depth is 0.
pub fn order(net: &Network, geom: &Geometry) -> Vec<Fragment> {
    let mut out = Vec::new();
    // Depth keys are distances: far is larger, and drawn first.
    let key = |pass, class, z, order| Key { pass, layer: 0, depth: 0.0, class, z, order };
    // `0.0 - depth`, not `-depth`: 2D depths stay +0, as the others.
    let at_depth = |k: Key, depth: f64| Key { depth: 0.0 - depth, ..k };
    for t in &geom.tensors {
        let key =
            |pass, class, z, order| Key { layer: t.layer, ..at_depth(key(pass, class, z, order), t.depth) };
        let attrs = net.tensor_style(t.tensor);
        let z = number(&attrs, "z").unwrap_or(0.0);
        let order = t.tensor.0 as f64;
        if word(&attrs, "shadow") != Some("off") {
            out.push(Fragment {
                key: key(Pass::Scene, Class::Shadows, z, order),
                part: Part::TensorShadow(t.tensor),
            });
        }
        out.push(Fragment { key: key(Pass::Scene, Class::Tensors, z, order), part: Part::Tensor(t.tensor) });
    }
    let line_key = |k: usize| {
        let l = &geom.lines[k];
        let attrs = match l.kind {
            LineKind::Bond => net.bond_style(l.index),
            LineKind::Leg => {
                let (t, s) = net.index(l.index).holders()[0];
                net.leg_style(t, s)
            }
        };
        let class = if word(&attrs, "layer") == Some("front") { Class::Front } else { Class::Bonds };
        key(Pass::Scene, class, number(&attrs, "z").unwrap_or(0.0), l.index.0 as f64)
    };
    // In 3D, the key of the span of line `k` at a point of the page.
    let span_key = |k: usize, p: V3| {
        let line = &geom.lines[k];
        let s = line.centerline.nearest_arc(p);
        let span = line.spans.iter().find(|sp| s <= sp.to + 1e-9).or(line.spans.last()).unwrap();
        Key { layer: span.layer, ..at_depth(line_key(k), span.depth) }
    };
    for (k, plane) in geom.planes.iter().enumerate() {
        let z = number(&net.plane_style(k), "z").unwrap_or(0.0);
        let key = Key {
            layer: plane.layer,
            ..at_depth(key(Pass::Scene, Class::Tensors, z, k as f64), plane.depth)
        };
        out.push(Fragment { key, part: Part::Plane(k) });
    }
    for (k, line) in geom.lines.iter().enumerate() {
        if line.spans.is_empty() {
            out.push(Fragment { key: line_key(k), part: Part::Line(k) });
        } else {
            for (i, span) in line.spans.iter().enumerate() {
                let key = Key { layer: span.layer, ..at_depth(line_key(k), span.depth) };
                out.push(Fragment { key, part: Part::Span(k, i) });
            }
        }
        if let Some(arrow) = &line.arrow {
            let key = if line.spans.is_empty() { line_key(k) } else { span_key(k, arrow_middle(arrow)) };
            out.push(Fragment { key: Key { order: key.order + 0.25, ..key }, part: Part::Arrow(k) });
        }
    }
    for (k, label) in geom.labels.iter().enumerate() {
        let key = match label.owner {
            // A tensor's label is printed on it, right after it.
            LabelOwner::Tensor(t) => {
                let body = out.iter().find(|f| f.part == Part::Tensor(t)).unwrap().key;
                Key { order: body.order + 0.5, ..body }
            }
            // Printed on a line: ordered with the line, or its span.
            LabelOwner::Line(l) if label.on_line => {
                let line = if geom.lines[l].spans.is_empty() { line_key(l) } else { span_key(l, label.pos) };
                Key { order: line.order + 0.5, ..line }
            }
            // Beside a line: above the whole scene.
            LabelOwner::Line(l) => Key { pass: Pass::Overlay, class: Class::Bonds, ..line_key(l) },
        };
        out.push(Fragment { key, part: Part::Label(k) });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// The middle of an arrow on the page.
fn arrow_middle(arrow: &ArrowGeom) -> V3 {
    let (ArrowGeom::Flat { shape: path, .. } | ArrowGeom::Cone { outline: path, .. }) = arrow;
    let pts = path.sample(0.05);
    pts.iter().fold(V3::ZERO, |a, p| a + *p) * (1.0 / pts.len().max(1) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GeometryOptions, LabelSizes, geometry, layout, parse};

    fn parts(src: &str) -> Vec<Part> {
        let net = parse(src).unwrap();
        let lay = layout(&net).unwrap();
        let geom = geometry(&net, &lay, &GeometryOptions::default(), &LabelSizes::new()).unwrap();
        order(&net, &geom).into_iter().map(|f| f.part).collect()
    }

    #[test]
    fn bonds_then_shadows_then_tensors_then_front() {
        let p = parts(
            "A at (0, 0)\nB at (3, 0)\nA - B\nA.f - B.f [layer=front]\nA [label=none]\nB [label=none]\n",
        );
        let t = |k| TensorId(k);
        assert_eq!(
            p,
            [
                Part::Line(0),
                Part::TensorShadow(t(0)),
                Part::TensorShadow(t(1)),
                Part::Tensor(t(0)),
                Part::Tensor(t(1)),
                Part::Line(1)
            ]
        );
    }

    #[test]
    fn labels_follow_their_owners() {
        let p = parts("A at (0, 0)\nB at (3, 0)\nA - B [label=$k$]\nA [z=1]\n");
        // B's label right after B; A (z=1) after B; the bond label beside
        // the bond, in the overlay, last.
        let pos = |q: Part| p.iter().position(|x| *x == q).unwrap();
        assert!(pos(Part::Tensor(TensorId(1))) < pos(Part::Tensor(TensorId(0))));
        assert_eq!(pos(Part::Tensor(TensorId(1))) + 1, pos(Part::Label(1)));
        assert_eq!(*p.last().unwrap(), Part::Label(2));
    }
}
