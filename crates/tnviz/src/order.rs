//! The order stage (docs/language.md, section 8.9): the pieces of a
//! figure, sorted into drawing order by (pass, depth, class, z, order).

use std::cmp::Ordering;

use crate::geometry::{Geometry, LabelOwner, LineKind};
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
    /// In 3D, minus the rank from the compositor (language section 11.7),
    /// and 1 for shadows, under everything; 0 in 2D.
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

/// The fragments of a figure in drawing order.  In 2D every depth is 0; in
/// 3D the compositor has ranked every object, and shadows come first.
pub fn order(net: &Network, geom: &Geometry) -> Vec<Fragment> {
    let mut out = Vec::new();
    let three = geom.frame.is_some();
    let key = |pass, class, z, order| Key { pass, depth: 0.0, class, z, order };
    // `0.0 - rank`, not `-rank`: in 2D depths stay +0, as the others.
    let ranked = |k: Key, rank: u32| if three { Key { depth: 0.0 - rank as f64, ..k } } else { k };
    for t in &geom.tensors {
        let attrs = net.tensor_style(t.tensor);
        let z = number(&attrs, "z").unwrap_or(0.0);
        let order = t.tensor.0 as f64;
        if word(&attrs, "shadow") != Some("off") {
            let key = key(Pass::Scene, Class::Shadows, z, order);
            let key = if three { Key { depth: 1.0, ..key } } else { key };
            out.push(Fragment { key, part: Part::TensorShadow(t.tensor) });
        }
        let key = ranked(key(Pass::Scene, Class::Tensors, z, order), t.drawn.rank);
        out.push(Fragment { key, part: Part::Tensor(t.tensor) });
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
        ranked(key(Pass::Scene, class, number(&attrs, "z").unwrap_or(0.0), l.index.0 as f64), l.drawn.rank)
    };
    for (k, plane) in geom.planes.iter().enumerate() {
        let z = number(&net.plane_style(k), "z").unwrap_or(0.0);
        let key = ranked(key(Pass::Scene, Class::Tensors, z, k as f64), plane.drawn.rank);
        out.push(Fragment { key, part: Part::Plane(k) });
    }
    for (k, line) in geom.lines.iter().enumerate() {
        out.push(Fragment { key: line_key(k), part: Part::Line(k) });
        if line.arrow.is_some() {
            let key = line_key(k);
            out.push(Fragment { key: Key { order: key.order + 0.25, ..key }, part: Part::Arrow(k) });
        }
    }
    for (k, label) in geom.labels.iter().enumerate() {
        let key = match label.owner {
            // In 3D, every label is a card above the scene, far first
            // (language section 11.7).
            _ if three => ranked(key(Pass::Overlay, Class::Bonds, 0.0, k as f64), label.drawn.rank),
            // A tensor's label is printed on it, right after it.
            LabelOwner::Tensor(t) => {
                let body = out.iter().find(|f| f.part == Part::Tensor(t)).unwrap().key;
                Key { order: body.order + 0.5, ..body }
            }
            // Printed on a line: ordered with the line.
            LabelOwner::Line(l) if label.on_line => {
                let line = line_key(l);
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
