//! The attribute registry of docs/language.md, section 10.3: every
//! attribute's category, the objects it applies to, and its type.
//!
//! The front end validates every rule against the registry, and the stages
//! read attributes through the typed accessors at the end of this module.

use crate::error::{Error, Result};
use crate::model::{Direction, Selector};
use crate::value::{Attr, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Structure,
    Layout,
    Geometry,
    Appearance,
    Order,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Tensor,
    Bond,
    Leg,
    /// A translucent plane (3D).
    Plane,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Number,
    /// A number between the bounds, inclusive.
    Fraction,
    /// A length; unitless values are layout units.
    Length,
    /// A length; unitless values are em.
    EmLength,
    Direction,
    Points,
    Point,
    Colour,
    /// A colour or `auto`.
    ColourOrAuto,
    /// Text, or a label generated from the index.
    LabelText,
    /// Text or `none`.
    TensorLabel,
    /// A backend font, such as a LaTeX font switch.
    Font,
    Word(&'static [&'static str]),
    /// An angle, or three angles `(rx, ry, rz)` for 3D.
    Rotation,
}

struct Spec {
    key: &'static str,
    targets: &'static [Target],
    category: Category,
    kind: Kind,
}

use Category::*;
use Kind::*;
use Target::*;

const T: &[Target] = &[Tensor];
const B: &[Target] = &[Bond];
const L: &[Target] = &[Leg];
const BL: &[Target] = &[Bond, Leg];
const TP: &[Target] = &[Tensor, Plane];

const SPECS: &[Spec] = &[
    // Tensors.
    Spec {
        key: "shape",
        targets: T,
        category: Geometry,
        kind: Word(&["rect", "orb", "triangle", "diamond", "dot", "sphere", "box", "prism", "octahedron"]),
    },
    Spec { key: "width", targets: &[Tensor, Bond, Leg, Plane], category: Geometry, kind: Length },
    Spec { key: "height", targets: TP, category: Geometry, kind: Length },
    Spec { key: "corner-radius", targets: TP, category: Geometry, kind: Length },
    Spec { key: "rotate", targets: TP, category: Layout, kind: Rotation },
    Spec { key: "padding", targets: &[Plane], category: Geometry, kind: Length },
    Spec { key: "thickness", targets: T, category: Geometry, kind: Length },
    Spec { key: "color", targets: &[Tensor, Bond, Leg, Plane], category: Appearance, kind: Colour },
    Spec { key: "label", targets: T, category: Appearance, kind: TensorLabel },
    Spec { key: "label-padding", targets: T, category: Geometry, kind: EmLength },
    Spec { key: "highlight-size", targets: T, category: Appearance, kind: Number },
    Spec { key: "highlight-inset", targets: T, category: Appearance, kind: Length },
    Spec { key: "shadow", targets: T, category: Appearance, kind: Word(&["on", "off"]) },
    Spec { key: "opacity", targets: &[Tensor, Bond, Leg, Plane], category: Appearance, kind: Fraction },
    Spec { key: "z", targets: &[Tensor, Bond, Plane], category: Order, kind: Number },
    Spec { key: "label-color", targets: &[Tensor, Bond, Leg], category: Appearance, kind: ColourOrAuto },
    Spec { key: "label-font", targets: &[Tensor, Bond, Leg], category: Appearance, kind: Font },
    // Bonds and legs.
    Spec { key: "style", targets: BL, category: Geometry, kind: Word(&["line", "tube"]) },
    Spec { key: "via", targets: B, category: Layout, kind: Points },
    Spec { key: "bend-radius", targets: BL, category: Geometry, kind: Length },
    Spec { key: "stub", targets: B, category: Geometry, kind: Length },
    Spec { key: "cap", targets: BL, category: Geometry, kind: Word(&["round", "flat"]) },
    Spec { key: "parallel-gap", targets: B, category: Geometry, kind: Length },
    Spec { key: "loop-size", targets: B, category: Geometry, kind: Length },
    Spec { key: "crossing", targets: B, category: Geometry, kind: Word(&["none", "hop"]) },
    Spec { key: "hop-radius", targets: B, category: Geometry, kind: Length },
    Spec { key: "arrow", targets: BL, category: Geometry, kind: Word(&["none", "forward", "backward"]) },
    Spec { key: "arrow-pos", targets: BL, category: Geometry, kind: Fraction },
    Spec {
        key: "arrow-style",
        targets: BL,
        category: Geometry,
        kind: Word(&["head", "shaft", "beside", "cone"]),
    },
    Spec { key: "arrow-size", targets: BL, category: Geometry, kind: Number },
    Spec { key: "layer", targets: B, category: Order, kind: Word(&["back", "front"]) },
    Spec { key: "leg-dir", targets: L, category: Layout, kind: Direction },
    Spec { key: "leg-length", targets: L, category: Geometry, kind: Length },
    Spec { key: "leg-offset", targets: L, category: Geometry, kind: Length },
    Spec { key: "label-pos", targets: BL, category: Geometry, kind: Fraction },
    Spec { key: "label-placement", targets: BL, category: Geometry, kind: Word(&["on", "beside"]) },
    Spec { key: "label-side", targets: BL, category: Geometry, kind: Word(&["auto", "left", "right"]) },
    Spec { key: "label-along", targets: BL, category: Geometry, kind: Length },
    Spec { key: "label-offset", targets: BL, category: Geometry, kind: Length },
    Spec { key: "label-shift", targets: BL, category: Geometry, kind: Point },
    Spec { key: "label-distance", targets: BL, category: Geometry, kind: EmLength },
    Spec { key: "start-label", targets: BL, category: Appearance, kind: LabelText },
    Spec { key: "end-label", targets: B, category: Appearance, kind: LabelText },
    Spec { key: "end-label-inset", targets: BL, category: Geometry, kind: EmLength },
    Spec { key: "end-label-side", targets: BL, category: Geometry, kind: Word(&["auto", "left", "right"]) },
];

/// Label text for bonds and legs, besides literal text.
const LABEL_WORDS: &[&str] = &["dim", "name", "dim+prime"];

/// The same key names a bond label and a tensor label; bonds and legs
/// accept the generated forms too.
fn spec(key: &str, target: Target) -> Option<&'static Spec> {
    if key == "label" && target != Tensor {
        return Some(&Spec { key: "label", targets: BL, category: Appearance, kind: LabelText });
    }
    SPECS.iter().find(|s| s.key == key && s.targets.contains(&target))
}

/// The category of an attribute on a target, if the registry knows it.
pub fn category(key: &str, target: Target) -> Option<Category> {
    spec(key, target).map(|s| s.category)
}

/// Shorthand flags and what they stand for.
const FLAGS: &[(&str, &str, &str)] =
    &[("tube", "style", "tube"), ("line", "style", "line"), ("hop", "crossing", "hop")];

/// Replace shorthand flags by the attributes they stand for.
pub(crate) fn expand_flags(attrs: &[Attr]) -> Result<Vec<Attr>> {
    attrs
        .iter()
        .map(|a| match a.value {
            Value::Flag => FLAGS
                .iter()
                .find(|(flag, _, _)| *flag == a.key)
                .map(|(_, key, value)| Attr::new(*key, Value::Word((*value).into())))
                .ok_or_else(|| Error::new(format!("`{}` needs a value", a.key))),
            _ => Ok(a.clone()),
        })
        .collect()
}

/// The targets a selector may pick.
pub(crate) fn targets(selector: &Selector) -> &'static [Target] {
    match selector {
        Selector::Tensors | Selector::Tensor(_) => T,
        Selector::Bonds => B,
        Selector::Legs | Selector::OpenLegs | Selector::LegOf(..) | Selector::Slot(..) => L,
        Selector::Tag(_) | Selector::Index(_) => BL,
        Selector::Name(_) => &[Tensor, Bond, Leg],
        Selector::Planes => &[Plane],
        Selector::GroupBonds(_) => B,
        Selector::GroupLegs(_) => L,
    }
}

/// Check that every attribute is known for some target, and that its value
/// has the right type.
pub(crate) fn validate(targets: &[Target], attrs: &[Attr]) -> Result<()> {
    for a in attrs {
        let specs: Vec<&Spec> = targets.iter().filter_map(|t| spec(&a.key, *t)).collect();
        if specs.is_empty() {
            let known_elsewhere = [Tensor, Bond, Leg, Plane].iter().any(|t| spec(&a.key, *t).is_some());
            return Err(Error::new(if known_elsewhere {
                format!("`{}` does not apply here", a.key)
            } else {
                format!("unknown attribute `{}`", a.key)
            }));
        }
        // The value must fit the attribute on at least one target.
        if let Err(problem) = specs.iter().map(|s| check(s.kind, &a.value)).reduce(|a, b| a.or(b)).unwrap() {
            return Err(Error::new(format!("`{}`: {problem}", a.key)));
        }
    }
    Ok(())
}

fn check(kind: Kind, v: &Value) -> std::result::Result<(), String> {
    let ok = match (kind, v) {
        (Number | Rotation, Value::Number(_)) => true,
        (Rotation, Value::Points(p)) => p.len() == 1 && p[0].len() == 3,
        (Fraction, Value::Number(x)) => (0.0..=1.0).contains(x),
        (Length | EmLength, Value::Number(_) | Value::Length(..)) => true,
        (Direction, Value::Number(_)) => true,
        (Direction, Value::Word(w)) => direction_word(w).is_some(),
        (Direction | Point, Value::Points(p)) => p.len() == 1,
        (Points, Value::Points(_)) => true,
        (Colour | ColourOrAuto, Value::Word(_) | Value::Str(_)) => true,
        (LabelText | TensorLabel, Value::Math(_) | Value::Str(_)) => true,
        (LabelText, Value::Word(w)) => LABEL_WORDS.contains(&w.as_str()),
        (TensorLabel, Value::Word(w)) => w == "none",
        (Font, Value::Word(_) | Value::Str(_)) => true,
        (Word(choices), Value::Word(w)) => choices.contains(&w.as_str()),
        _ => false,
    };
    if ok { Ok(()) } else { Err(format!("expected {}, found `{v}`", describe(kind))) }
}

fn describe(kind: Kind) -> String {
    match kind {
        Number => "a number".into(),
        Fraction => "a number between 0 and 1".into(),
        Length | EmLength => "a length".into(),
        Rotation => "an angle or three angles (rx, ry, rz)".into(),
        Direction => "a direction".into(),
        Points => "points".into(),
        Point => "a point".into(),
        Colour => "a colour".into(),
        ColourOrAuto => "a colour or `auto`".into(),
        LabelText => "text, `dim`, `name`, or `dim+prime`".into(),
        TensorLabel => "text or `none`".into(),
        Font => "a font".into(),
        Word(choices) => format!("one of {}", choices.join(", ")),
    }
}

fn direction_word(w: &str) -> Option<Direction> {
    if Direction::WORDS.contains(&w) {
        Some(Direction::Word(w.to_string()))
    } else if matches!(w, "+x" | "-x" | "+y" | "-y" | "+z" | "-z") {
        Some(Direction::Axis(w.to_string()))
    } else {
        None
    }
}

// ---- Typed accessors -------------------------------------------------------
//
// Values have been validated, so an accessor returns `None` only when the
// attribute is not set.

pub(crate) fn get<'a>(attrs: &'a [Attr], key: &str) -> Option<&'a Value> {
    attrs.iter().find(|a| a.key == key).map(|a| &a.value)
}

pub(crate) fn word<'a>(attrs: &'a [Attr], key: &str) -> Option<&'a str> {
    match get(attrs, key)? {
        Value::Word(w) => Some(w),
        _ => None,
    }
}

pub(crate) fn number(attrs: &[Attr], key: &str) -> Option<f64> {
    match get(attrs, key)? {
        Value::Number(x) => Some(*x),
        _ => None,
    }
}

/// A rotation as angles about x, y, and z, in degrees; one angle is about z.
pub(crate) fn rotation(attrs: &[Attr], key: &str) -> Option<[f64; 3]> {
    match get(attrs, key)? {
        Value::Number(x) => Some([0.0, 0.0, *x]),
        Value::Points(p) => Some([p[0][0], p[0][1], p[0][2]]),
        _ => None,
    }
}

pub(crate) fn direction(attrs: &[Attr], key: &str) -> Option<Direction> {
    match get(attrs, key)? {
        Value::Number(a) => Some(Direction::Angle(*a)),
        Value::Word(w) => direction_word(w),
        Value::Points(p) if p.len() == 1 => Some(Direction::Vector(p[0].clone())),
        _ => None,
    }
}

pub(crate) fn points(attrs: &[Attr], key: &str) -> Option<Vec<Vec<f64>>> {
    match get(attrs, key)? {
        Value::Points(p) => Some(p.clone()),
        _ => None,
    }
}
