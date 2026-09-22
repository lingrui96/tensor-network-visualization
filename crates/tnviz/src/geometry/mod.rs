//! The geometry stage (docs/language.md, section 8): tensor outlines, bond
//! and leg centrelines with their fillets and hops, tube outlines, visible
//! lengths, crossings, and label positions, in layout units.
//!
//! Lighting and drawing order are later stages; this one only builds
//! shapes.  Only 2D scenes are specified.

mod path;
mod shape;

use std::collections::{BTreeMap, HashMap};

pub use path::{Cap, Path, Piece};
pub use shape::Shape;

use crate::error::{Error, Result};
use crate::layout::{Placement, Route, V3};
use crate::model::{Dim, IndexId, Network, TensorId, index_display};
use crate::registry::{self, get, number, word};
use crate::value::{Attr, Value};
use path::{Filleted, Vertex, fillet_open, tube_outline};

/// Sizes that geometry needs from the outer layer.
#[derive(Clone, Debug)]
pub struct GeometryOptions {
    /// TeX points per layout unit (28.45274 for 1cm).
    pub unit_pt: f64,
    /// TeX points per em of the label font.
    pub em_pt: f64,
}

impl Default for GeometryOptions {
    fn default() -> Self {
        GeometryOptions { unit_pt: 28.45274, em_pt: 10.0 }
    }
}

/// Typeset label sizes in TeX points, keyed by label id (docs/protocol.md).
#[derive(Clone, Debug, Default)]
pub struct LabelSizes {
    sizes: HashMap<String, [f64; 3]>,
}

impl LabelSizes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: impl Into<String>, width: f64, height: f64, depth: f64) {
        self.sizes.insert(id.into(), [width, height, depth]);
    }

    pub fn get(&self, id: &str) -> Option<[f64; 3]> {
        self.sizes.get(id).copied()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TensorGeom {
    pub tensor: TensorId,
    pub shape: Shape,
    pub center: V3,
    pub rotation: f64,
    pub width: f64,
    pub height: f64,
    /// The corner radius after clamping.
    pub corner_radius: f64,
    /// The silhouette, in page coordinates.
    pub outline: Path,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStyle {
    Line,
    Tube,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Bond,
    Leg,
}

/// A bond or an open leg.
#[derive(Clone, Debug, PartialEq)]
pub struct LineGeom {
    pub index: IndexId,
    pub kind: LineKind,
    pub style: LineStyle,
    /// Stroke width of a line, or diameter of a tube, in layout units.
    pub width: f64,
    pub centerline: Path,
    /// The outline of a tube.
    pub outline: Option<Path>,
    /// Caps at the start and end.
    pub caps: (Cap, Cap),
    /// The arc-length range outside the end tensors' silhouettes.
    pub visible: (f64, f64),
    /// The arrow (section 8.11).
    pub arrow: Option<ArrowGeom>,
}

/// An arrow on or beside a line (section 8.11).
#[derive(Clone, Debug, PartialEq)]
pub enum ArrowGeom {
    /// A flat shape, filled: a head alone or with a shaft, on the line, or
    /// beside it when `beside`.
    Flat { shape: Path, beside: bool },
    /// A cone ending the tube, which stops at its base: the tube itself is
    /// the arrow.  The tip touches the tensor it points to, or ends the leg.
    /// `base_at` is the base's arc length along the centreline, and
    /// `forward` whether the cone points along it.
    Cone { outline: Path, base: V3, axis: V3, length: f64, radius: f64, base_at: f64, forward: bool },
}

/// Where a label's box is attached.
#[derive(Clone, Debug, PartialEq)]
pub enum Anchor {
    /// The box's centre is at the position.
    Center,
    /// The point of the box's border in this direction is at the position.
    Toward(V3),
}

/// What a label belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelOwner {
    Tensor(TensorId),
    /// A bond or leg, by its position in `Geometry::lines`.
    Line(usize),
}

/// A label's text, as the language writes it: TeX math with its dollars,
/// or plain text, which a backend prints literally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LabelText {
    Math(String),
    Plain(String),
}

impl LabelText {
    pub fn as_str(&self) -> &str {
        match self {
            LabelText::Math(s) | LabelText::Plain(s) => s,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelGeom {
    /// The label id of docs/protocol.md, such as `t:A[1]` or `b:_link[3]`.
    pub id: String,
    pub owner: LabelOwner,
    /// For a line's label: whether it is printed on the line, rather than
    /// beside it.
    pub on_line: bool,
    pub text: LabelText,
    pub pos: V3,
    /// Rotation in degrees.
    pub angle: f64,
    pub anchor: Anchor,
    /// Width, height, and depth in layout units.
    pub size: (f64, f64, f64),
    /// Whether the size was measured, rather than estimated.
    pub measured: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Crossing {
    pub point: V3,
    pub upper: IndexId,
    pub lower: IndexId,
    pub hopped: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    /// Indexed by `TensorId`.
    pub tensors: Vec<TensorGeom>,
    /// Bonds, then open legs.
    pub lines: Vec<LineGeom>,
    pub labels: Vec<LabelGeom>,
    pub crossings: Vec<Crossing>,
    pub warnings: Vec<String>,
}

/// Build the geometry of a placed network.
pub fn geometry(
    net: &Network,
    lay: &Placement,
    opts: &GeometryOptions,
    sizes: &LabelSizes,
) -> Result<Geometry> {
    if net.scene().dim == Dim::Three {
        return Err(Error::new("geometry is specified for 2D scenes only"));
    }
    let mut g = Builder { net, lay, opts, sizes, warnings: Vec::new() };
    let (tensors, mut labels) = g.tensors()?;
    let mut lines = Vec::new();
    let mut bonds = Vec::new();
    for b in &lay.bonds {
        let bond = g.bond(b, &tensors);
        lines.push(bond.line.clone());
        bonds.push(bond);
    }
    let offsets = g.leg_offsets(&lay.legs, &tensors);
    for (leg, &(offset, side)) in lay.legs.iter().zip(&offsets) {
        lines.push(g.leg(leg, &tensors, tensors[leg.tensor.0].center + side * offset));
    }
    let (mut crossings, upper_pieces) = g.crossings(&lines, &tensors);
    g.hops(&mut bonds, &mut lines, &mut crossings, &upper_pieces);
    for line in &mut lines {
        line.visible = g.visible(line, &tensors);
    }
    for (k, line) in lines.iter().enumerate() {
        if let Some(label) = g.line_label(k, line) {
            labels.push(label);
        }
    }
    for (k, line) in lines.iter_mut().enumerate() {
        let label = labels.iter().find(|l| l.owner == LabelOwner::Line(k));
        line.arrow = g.arrow(line, label);
        // A cone replaces the end of its tube.
        if let (Some(ArrowGeom::Cone { base_at, forward, .. }), Some(_)) = (&line.arrow, &line.outline) {
            let total = line.centerline.length();
            let (s0, s1, caps) = if *forward {
                (0.0, *base_at, (line.caps.0, Cap::Flat))
            } else {
                (*base_at, total, (Cap::Flat, line.caps.1))
            };
            line.outline = Some(tube_outline(&line.centerline.slice(s0, s1), line.width / 2.0, caps));
        }
    }
    Ok(Geometry { tensors, lines, labels, crossings, warnings: g.warnings })
}

/// A hop to insert on a polyline segment.
#[derive(Clone)]
struct PlannedHop {
    /// Distance from the segment's start.
    along: f64,
    point: V3,
    radius: f64,
    crossing: usize,
}

/// A bond with what is needed to rebuild it with hops.
struct BondBuild {
    line: LineGeom,
    vertices: Vec<Vertex>,
    filleted: Filleted,
    reference_width: f64,
    bend: f64,
    hop: bool,
    hop_radius: Option<f64>,
}

struct Builder<'a> {
    net: &'a Network,
    lay: &'a Placement,
    opts: &'a GeometryOptions,
    sizes: &'a LabelSizes,
    warnings: Vec<String>,
}

fn left(d: V3) -> V3 {
    V3::xy(-d.y, d.x)
}

/// The arrow style of tubes without `arrow-style` (section 8.11).
const TUBE_ARROW: &str = "cone";

/// A flat arrow along a centreline, centred at arc length `s` and moved
/// sideways by `offset`: a head `head` long and `wide` wide, and before it a
/// shaft `shaft` wide filling the rest of `total`.  The shaft follows the
/// centreline's bends.
#[allow(clippy::too_many_arguments)]
fn flat_arrow(
    path: &Path,
    s: f64,
    sense: f64,
    total: f64,
    head: f64,
    wide: f64,
    shaft: f64,
    offset: V3,
) -> Path {
    let at = |u: f64| {
        let (p, t) = path.at(u);
        (p + offset, t * sense)
    };
    let tip = s + sense * total / 2.0;
    let head_base = tip - sense * head;
    let (hb, ht) = at(head_base);
    let hn = left(ht);
    let mut pts = Vec::new();
    let shaft_len = total - head;
    let samples = if shaft_len > 1e-9 && shaft > 1e-9 { 12 } else { 0 };
    let tail = s - sense * total / 2.0;
    let along: Vec<(V3, V3)> =
        (0..=samples).map(|i| at(tail + (head_base - tail) * i as f64 / samples.max(1) as f64)).collect();
    if samples > 0 {
        pts.extend(along.iter().map(|(p, t)| *p + left(*t) * (shaft / 2.0)));
    }
    pts.push(hb + hn * (wide / 2.0));
    pts.push(at(tip).0);
    pts.push(hb - hn * (wide / 2.0));
    if samples > 0 {
        pts.extend(along.iter().rev().map(|(p, t)| *p - left(*t) * (shaft / 2.0)));
    }
    let n = pts.len();
    Path { pieces: (0..n).map(|k| Piece::Line { a: pts[k], b: pts[(k + 1) % n] }).collect(), closed: true }
}

/// A name as TeX math, the way backends typeset it: `A[1,2]'` as
/// `$A_{1,2}'$`.
fn math_name(base: &str, subscripts: &[i64], prime: u32) -> String {
    let mut s = format!("${}", base.replace('_', "\\_"));
    if !subscripts.is_empty() {
        let subs: Vec<String> = subscripts.iter().map(|s| s.to_string()).collect();
        s += &format!("_{{{}}}", subs.join(","));
    }
    s += &"'".repeat(prime as usize);
    s.push('$');
    s
}

/// A rough label size in em, before LaTeX has measured it: math commands
/// count as one character, and scripts shrink.
fn estimate(text: &str) -> (f64, f64, f64) {
    let mut chars = 0.0f64;
    let mut script = false;
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '$' | '{' | '}' | ' ' => {}
            '^' | '_' => script = true,
            '\\' => {
                // A command word, or a single escaped character such as `\_`.
                if it.next_if(|c| c.is_ascii_alphabetic()).is_some() {
                    while it.next_if(|c| c.is_ascii_alphabetic()).is_some() {}
                } else {
                    it.next();
                }
                chars += if script { 0.7 } else { 1.0 };
            }
            _ => chars += if script { 0.7 } else { 1.0 },
        }
    }
    let depth = if text.contains('_') { 0.3 } else { 0.2 };
    ((0.55 * chars).max(0.5), 0.7, depth)
}

impl Builder<'_> {
    fn em(&self) -> f64 {
        self.opts.em_pt / self.opts.unit_pt
    }

    /// A length attribute in layout units; unitless values are layout units,
    /// or em when `em_default`.
    fn length(&self, attrs: &[Attr], key: &str, em_default: bool) -> Option<f64> {
        let pt = |x: f64| x / self.opts.unit_pt;
        Some(match get(attrs, key)? {
            Value::Number(x) if em_default => x * self.em(),
            Value::Number(x) => *x,
            Value::Length(x, unit) => match unit.as_str() {
                "em" => x * self.em(),
                "pt" => pt(*x),
                "bp" => pt(x * 72.27 / 72.0),
                "mm" => pt(x * 72.27 / 25.4),
                "cm" => pt(x * 72.27 / 2.54),
                "in" => pt(x * 72.27),
                "px" => pt(x * 0.75 * 72.27 / 72.0),
                _ => return None,
            },
            _ => return None,
        })
    }

    /// Size of a label in layout units, measured or estimated.
    fn label_size(&self, id: &str, text: &str) -> ((f64, f64, f64), bool) {
        match self.sizes.get(id) {
            Some([w, h, d]) => {
                let u = self.opts.unit_pt;
                ((w / u, h / u, d / u), true)
            }
            None => {
                let (w, h, d) = estimate(text);
                let em = self.em();
                ((w * em, h * em, d * em), false)
            }
        }
    }

    // ---- Tensors --------------------------------------------------------

    fn tensors(&mut self) -> Result<(Vec<TensorGeom>, Vec<LabelGeom>)> {
        let mut out = Vec::new();
        let mut labels = Vec::new();
        for (id, t) in self.net.tensors() {
            let style = self.net.tensor_style(id);
            let shape_word = word(&style, "shape").unwrap_or("rect");
            let shape = Shape::from_word(shape_word).ok_or_else(|| {
                Error::new(format!("{}: `{shape_word}` is a 3D shape, in a 2D scene", t.name))
            })?;
            // The label: the name unless `none`, or a dot.
            // Estimates see the name as backends typeset it, with its
            // subscripts as a script: `A[1]` as A₁.
            let text = match get(&style, "label") {
                Some(Value::Word(w)) if w == "none" => None,
                Some(Value::Math(s)) => Some(LabelText::Math(s.clone())),
                Some(Value::Str(s)) => Some(LabelText::Plain(s.clone())),
                _ if shape == Shape::Dot => None,
                _ => Some(LabelText::Math(math_name(&t.name.base, &t.name.subscripts, 0))),
            };
            let label_id = format!("t:{}", t.name);
            let label = text.map(|text| (self.label_size(&label_id, text.as_str()), text));
            let pad = self.length(&style, "label-padding", true).unwrap_or(0.3 * self.em());
            let (a, b) =
                label.as_ref().map_or((0.0, 0.0), |(((w, h, d), _), _)| (w / 2.0 + pad, (h + d) / 2.0 + pad));

            let (mut w, mut h) = shape.default_size();
            let (ew, eh) = (self.length(&style, "width", false), self.length(&style, "height", false));
            if ew.is_none() && eh.is_none() {
                (w, h) = shape.fit((w, h), a, b);
            } else {
                w = ew.unwrap_or(w);
                h = if shape.is_round() { w } else { eh.unwrap_or(h) };
                if label.is_some() && !shape.fits(w, h, a, b) {
                    self.warnings.push(format!("{}: the label does not fit the tensor", t.name));
                }
            }
            let corner = self.length(&style, "corner-radius", false).unwrap_or(0.12);
            let placed = &self.lay.tensors[id.0];
            let (local, used) = shape.outline(w, h, corner);
            out.push(TensorGeom {
                tensor: id,
                shape,
                center: placed.pos,
                rotation: placed.rotation,
                width: w,
                height: h,
                corner_radius: used,
                outline: local.transformed(placed.rotation, placed.pos),
            });
            if let Some(((size, measured), text)) = label {
                labels.push(LabelGeom {
                    id: label_id,
                    owner: LabelOwner::Tensor(id),
                    on_line: false,
                    text,
                    pos: placed.pos,
                    angle: 0.0,
                    anchor: Anchor::Center,
                    size,
                    measured,
                });
            }
        }
        Ok((out, labels))
    }

    fn boundary(&self, tensors: &[TensorGeom], t: TensorId, dir: V3) -> f64 {
        let g = &tensors[t.0];
        g.outline.ray_distance(g.center, dir).unwrap_or(0.0)
    }

    // ---- Bonds and legs -------------------------------------------------

    /// Style, width, and the width that defaults refer to (the tube
    /// diameter, or 0 for a line).
    fn line_style(&self, attrs: &[Attr]) -> (LineStyle, f64, f64) {
        if word(attrs, "style") == Some("tube") {
            let w = self.length(attrs, "width", false).unwrap_or(0.22);
            (LineStyle::Tube, w, w)
        } else {
            (LineStyle::Line, self.length(attrs, "width", true).unwrap_or(0.09 * self.em()), 0.0)
        }
    }

    fn bond(&mut self, b: &crate::layout::PlacedBond, tensors: &[TensorGeom]) -> BondBuild {
        let attrs = self.net.bond_style(b.index);
        let (style, width, wref) = self.line_style(&attrs);
        let bend = self.length(&attrs, "bend-radius", false).unwrap_or(0.2f64.max(0.8 * wref));
        let stub = self.length(&attrs, "stub", false).unwrap_or(bend + wref / 2.0);
        let gap = self.length(&attrs, "parallel-gap", false).unwrap_or(0.3f64.max(1.5 * wref));
        let loop_size = self.length(&attrs, "loop-size", false).unwrap_or(0.45);

        let (ca, cb) = (tensors[b.a.tensor.0].center, tensors[b.b.tensor.0].center);
        let mut points = vec![ca];
        if let Some(d) = b.a.dir {
            points.push(ca + d * (self.boundary(tensors, b.a.tensor, d) + stub));
        }
        match &b.route {
            Route::Direct => {}
            Route::Via(v) => points.extend(v),
            Route::Parallel { k, n } => {
                let side = *k as f64 - (*n - 1) as f64 / 2.0;
                let normal = left(cb - ca).unit().unwrap_or(V3::xy(0.0, 1.0));
                points.push((ca + cb) * 0.5 + normal * (side * gap));
            }
            Route::Loop { k } => {
                let t = &tensors[b.a.tensor.0];
                let y = t.height / 2.0 + (*k + 1) as f64 * loop_size;
                for x in [-0.35 * t.width, 0.35 * t.width] {
                    points.push(t.center + V3::xy(x, y).rotate_z(t.rotation));
                }
            }
        }
        if let Some(d) = b.b.dir {
            points.push(cb + d * (self.boundary(tensors, b.b.tensor, d) + stub));
        }
        points.push(cb);
        points.dedup_by(|p, q| (*p - *q).norm() < 1e-4);
        let vertices: Vec<Vertex> =
            points.into_iter().map(|p| Vertex { p, radius: bend, clamp: 0.5 }).collect();
        let filleted = fillet_open(&vertices);
        self.check_bends(b.index, style, width, &filleted);
        let caps = (Cap::Round, Cap::Round);
        let outline = (style == LineStyle::Tube).then(|| tube_outline(&filleted.path, width / 2.0, caps));
        BondBuild {
            line: LineGeom {
                index: b.index,
                kind: LineKind::Bond,
                style,
                width,
                centerline: filleted.path.clone(),
                outline,
                caps,
                visible: (0.0, 0.0),
                arrow: None,
            },
            vertices,
            filleted,
            reference_width: wref,
            bend,
            hop: word(&attrs, "crossing") == Some("hop"),
            hop_radius: self.length(&attrs, "hop-radius", false),
        }
    }

    fn check_bends(&mut self, index: IndexId, style: LineStyle, width: f64, f: &Filleted) {
        if style == LineStyle::Tube && f.radii.iter().any(|&r| r > 1e-9 && r < width / 2.0) {
            let i = self.net.index(index);
            self.warnings.push(format!(
                "bond {}: a bend is tighter than the tube's radius",
                index_display(&i.name, i.prime)
            ));
        }
    }

    /// Where open legs leave their tensors (section 8.5): for each leg, its
    /// sideways offset and the unit vector it is measured along.  Legs of a
    /// tensor that share a direction, and have no `leg-offset`, are spread
    /// evenly, in slot order, across the tensor.
    fn leg_offsets(&self, legs: &[crate::layout::PlacedLeg], tensors: &[TensorGeom]) -> Vec<(f64, V3)> {
        let mut out = Vec::with_capacity(legs.len());
        let mut groups: std::collections::BTreeMap<(usize, i64, i64), Vec<usize>> = Default::default();
        for (k, leg) in legs.iter().enumerate() {
            let g = &tensors[leg.tensor.0];
            // The perpendicular in reading order, in the local frame: to the
            // right, or downwards for horizontal legs.
            let d = leg.dir.rotate_z(-g.rotation);
            let mut p = V3::xy(-d.y, d.x);
            if p.x < -1e-9 || (p.x.abs() <= 1e-9 && p.y > 0.0) {
                p = -p;
            }
            let side = p.rotate_z(g.rotation);
            let attrs = self.net.leg_style(leg.tensor, leg.slot);
            match self.length(&attrs, "leg-offset", false) {
                Some(offset) => out.push((offset, side)),
                None => {
                    out.push((0.0, side));
                    let key = (leg.tensor.0, (d.x * 1e6).round() as i64, (d.y * 1e6).round() as i64);
                    groups.entry(key).or_default().push(k);
                }
            }
        }
        for members in groups.values_mut() {
            if members.len() < 2 {
                continue;
            }
            members.sort_by_key(|&k| legs[k].slot);
            let (t, side) = (legs[members[0]].tensor, out[members[0]].1);
            let g = &tensors[t.0];
            // The chord through the centre, less the corner radii.
            let r = g.corner_radius;
            let lo = -self.boundary(tensors, t, -side) + r;
            let hi = self.boundary(tensors, t, side) - r;
            let (lo, hi) = if hi > lo { (lo, hi) } else { (0.0, 0.0) };
            let n = members.len() as f64;
            for (i, &k) in members.iter().enumerate() {
                out[k].0 = lo + (i as f64 + 0.5) * (hi - lo) / n;
            }
        }
        out
    }

    fn leg(&mut self, leg: &crate::layout::PlacedLeg, tensors: &[TensorGeom], start: V3) -> LineGeom {
        let attrs = self.net.leg_style(leg.tensor, leg.slot);
        let (style, width, _) = self.line_style(&attrs);
        let length = self.length(&attrs, "leg-length", false).unwrap_or(0.6);
        let inside = tensors[leg.tensor.0].outline.ray_distance(start, leg.dir).unwrap_or(0.0);
        let end = start + leg.dir * (inside + length);
        let centerline = Path { pieces: vec![Piece::Line { a: start, b: end }], closed: false };
        let cap = if word(&attrs, "cap") == Some("flat") { Cap::Flat } else { Cap::Round };
        let caps = (Cap::Round, cap);
        let outline = (style == LineStyle::Tube).then(|| tube_outline(&centerline, width / 2.0, caps));
        LineGeom {
            index: leg.index,
            kind: LineKind::Leg,
            style,
            width,
            centerline,
            outline,
            caps,
            visible: (0.0, 0.0),
            arrow: None,
        }
    }

    /// The drawing key of section 8.9, without depth: (class, z, order).
    fn key(&self, line: &LineGeom) -> (u8, f64, usize) {
        let attrs = match line.kind {
            LineKind::Bond => self.net.bond_style(line.index),
            LineKind::Leg => {
                let (t, s) = self.net.index(line.index).holders()[0];
                self.net.leg_style(t, s)
            }
        };
        let class = if word(&attrs, "layer") == Some("front") { 1 } else { 0 };
        (class, number(&attrs, "z").unwrap_or(0.0), line.index.0)
    }

    fn inside_tensor(&self, p: V3, margin: f64, tensors: &[TensorGeom]) -> bool {
        tensors.iter().any(|t| {
            let d = p - t.center;
            let dist = d.norm();
            dist < 1e-9
                || d.unit()
                    .is_some_and(|u| dist < t.outline.ray_distance(t.center, u).unwrap_or(0.0) + margin)
        })
    }

    /// Crossings of centrelines outside every silhouette, each with the
    /// index of the upper line's piece it lies on.
    fn crossings(&self, lines: &[LineGeom], tensors: &[TensorGeom]) -> (Vec<Crossing>, Vec<usize>) {
        let mut out: Vec<Crossing> = Vec::new();
        let mut pieces = Vec::new();
        for (i, a) in lines.iter().enumerate() {
            for b in &lines[i + 1..] {
                let margin = (a.width + b.width) / 2.0;
                let a_up = self.key(a).partial_cmp(&self.key(b)) == Some(std::cmp::Ordering::Greater);
                let (upper, lower) = if a_up { (a, b) } else { (b, a) };
                for (ku, pu) in upper.centerline.pieces.iter().enumerate() {
                    for pl in &lower.centerline.pieces {
                        for (p, _, _) in pu.intersect(pl) {
                            let seen = out.iter().any(|c| {
                                (c.point - p).norm() < 1e-6
                                    && c.upper == upper.index
                                    && c.lower == lower.index
                            });
                            if !seen && !self.inside_tensor(p, margin, tensors) {
                                out.push(Crossing {
                                    point: p,
                                    upper: upper.index,
                                    lower: lower.index,
                                    hopped: false,
                                });
                                pieces.push(ku);
                            }
                        }
                    }
                }
            }
        }
        (out, pieces)
    }

    /// Rebuild the upper bond of every crossing whose upper bond hops
    /// (section 8.7).
    fn hops(
        &mut self,
        bonds: &mut [BondBuild],
        lines: &mut [LineGeom],
        crossings: &mut [Crossing],
        pieces: &[usize],
    ) {
        let tube_width = |id: IndexId, lines: &[LineGeom]| {
            lines
                .iter()
                .find(|l| l.index == id)
                .map_or(0.0, |l| if l.style == LineStyle::Tube { l.width } else { 0.0 })
        };
        // Planned hops, by bond and by polyline segment.
        let mut plan: HashMap<usize, BTreeMap<usize, Vec<PlannedHop>>> = HashMap::new();
        for (ci, c) in crossings.iter().enumerate() {
            let Some(k) = bonds.iter().position(|b| b.line.index == c.upper && b.hop) else { continue };
            let bond = &bonds[k];
            let Some(seg) = bond.filleted.segment[pieces[ci]] else {
                self.warnings.push("a crossing on a bend cannot hop; it is left alone".into());
                continue;
            };
            let radius = bond
                .hop_radius
                .unwrap_or(0.15f64.max(0.7 * (bond.reference_width + tube_width(c.lower, lines)) + 0.03));
            let along = (c.point - bond.vertices[seg].p).norm();
            plan.entry(k).or_default().entry(seg).or_default().push(PlannedHop {
                along,
                point: c.point,
                radius,
                crossing: ci,
            });
        }
        for (k, segments) in plan {
            let bond = &bonds[k];
            let base = if bond.line.style == LineStyle::Tube { bond.bend } else { 0.0 };
            let mut vertices = Vec::new();
            for (s, v) in bond.vertices.iter().enumerate() {
                vertices.push(v.clone());
                let Some(hops) = segments.get(&s) else { continue };
                let next = bond.vertices[s + 1].p;
                let seg_len = (next - v.p).norm();
                let e = (next - v.p).unit().unwrap();
                let mut n = left(e);
                if n.y < -1e-9 || (n.y.abs() <= 1e-9 && n.x > 0.0) {
                    n = -n;
                }
                let mut hops = hops.clone();
                hops.sort_by(|a, b| a.along.total_cmp(&b.along));
                let mut reach = 0.0;
                for PlannedHop { along, point: c, radius: h, crossing } in hops {
                    if along - h - base <= reach || along + h + base >= seg_len {
                        self.warnings
                            .push("a hop does not fit on its segment; the crossing is left alone".into());
                        continue;
                    }
                    reach = along + h + base;
                    let up = n * (h + base);
                    for (p, radius) in
                        [(c - e * h, base), (c - e * h + up, h), (c + e * h + up, h), (c + e * h, base)]
                    {
                        vertices.push(Vertex { p, radius, clamp: 1.0 });
                    }
                    crossings[crossing].hopped = true;
                }
            }
            let filleted = fillet_open(&vertices);
            let (index, style, width, caps) =
                (bond.line.index, bond.line.style, bond.line.width, bond.line.caps);
            self.check_bends(index, style, width, &filleted);
            let bond = &mut bonds[k];
            bond.line.centerline = filleted.path.clone();
            bond.line.outline =
                (style == LineStyle::Tube).then(|| tube_outline(&filleted.path, width / 2.0, caps));
            bond.vertices = vertices;
            bond.filleted = filleted;
            if let Some(line) = lines.iter_mut().find(|l| l.index == index && l.kind == LineKind::Bond) {
                *line = bond.line.clone();
            }
        }
    }

    /// The part of a centreline outside its end tensors (section 8.3).
    fn visible(&self, line: &LineGeom, tensors: &[TensorGeom]) -> (f64, f64) {
        let total = line.centerline.length();
        let holders = self.net.index(line.index).holders();
        // Measured from where the centreline starts: an offset leg starts
        // away from the centre.
        let (start, start_dir) = line.centerline.at(0.0);
        let first = &tensors[holders[0].0.0];
        let hidden_a = first.outline.ray_distance(start, start_dir).unwrap_or(0.0).min(total);
        let hidden_b = match line.kind {
            LineKind::Leg => 0.0,
            LineKind::Bond => {
                let (_, end_dir) = line.centerline.at(total);
                self.boundary(tensors, holders[1].0, -end_dir).min(total)
            }
        };
        (hidden_a, (total - hidden_b).max(hidden_a))
    }

    // ---- Labels and arrows ----------------------------------------------

    fn line_attrs(&self, line: &LineGeom) -> Vec<Attr> {
        match line.kind {
            LineKind::Bond => self.net.bond_style(line.index),
            LineKind::Leg => {
                let (t, s) = self.net.index(line.index).holders()[0];
                self.net.leg_style(t, s)
            }
        }
    }

    /// A line's arrow direction (1 forward, -1 backward) and style, with
    /// the defaults of section 8.11.
    fn arrow_style(&self, attrs: &[Attr], line: &LineGeom) -> Option<(f64, &'static str)> {
        let sense = match word(attrs, "arrow") {
            Some("forward") => 1.0,
            Some("backward") => -1.0,
            _ => return None,
        };
        let tube = line.style == LineStyle::Tube;
        let style = match word(attrs, "arrow-style") {
            Some("beside") => "beside",
            Some("shaft") if tube => "shaft",
            Some("cone") if tube => "cone",
            // `head`, or `shaft` and `cone` on a line.
            Some(_) => "head",
            None if tube => TUBE_ARROW,
            None => "head",
        };
        Some((sense, style))
    }

    /// The length of a cone arrow (section 8.11), if the line ends in one.
    fn cone_length(&self, attrs: &[Attr], line: &LineGeom) -> Option<(f64, f64)> {
        match self.arrow_style(attrs, line)? {
            (sense, "cone") => Some((sense, 1.7 * line.width * number(attrs, "arrow-size").unwrap_or(1.0))),
            _ => None,
        }
    }

    /// Where along a line its label sits, as arc length (section 8.8): on
    /// the visible part, less a cone arrow.
    fn label_arc(&self, attrs: &[Attr], line: &LineGeom) -> f64 {
        let (mut from, mut to) = line.visible;
        match self.cone_length(attrs, line) {
            Some((sense, len)) if sense > 0.0 => to -= len,
            Some((_, len)) => from += len,
            None => {}
        }
        from + number(attrs, "label-pos").unwrap_or(0.5) * (to - from)
            + self.length(attrs, "label-along", false).unwrap_or(0.0)
    }

    /// A line's arrow (section 8.11), pointing from the first holder to the
    /// second (for a leg, away from its tensor), or back for `backward`.
    /// Without `arrow-pos` it is centred on the visible part, or, for a
    /// head, just past a label printed on the line; beside the line, it
    /// keeps to the side away from a label there.
    fn arrow(&mut self, line: &LineGeom, label: Option<&LabelGeom>) -> Option<ArrowGeom> {
        let attrs = self.line_attrs(line);
        let (sense, style) = self.arrow_style(&attrs, line)?;
        let tube = line.style == LineStyle::Tube;
        let k = number(&attrs, "arrow-size").unwrap_or(1.0);
        let (d, em) = (line.width, self.em());
        // Head length and width, shaft width, and the length taken along
        // the line.
        let (head, wide, shaft, total): (f64, f64, f64, f64) = match (style, tube) {
            ("beside", _) => (0.55 * em * k, 0.5 * em * k, 0.09 * em, 2.4 * em * k),
            ("shaft", _) => (0.9 * d * k, 0.75 * d * k, 0.22 * d * k, 2.8 * d * k),
            (_, true) => (0.9 * d * k, 0.75 * d * k, 0.0, 0.9 * d * k),
            _ => (0.55 * em * k, 0.5 * em * k, 0.0, 0.55 * em * k),
        };
        let (from, to) = line.visible;
        if style == "cone" {
            return self.cone(line, sense, k, from, to);
        }
        // A shaft shortens to .8 of the visible part, down to 1.6 heads.
        let total = if shaft > 0.0 { total.min(0.8 * (to - from)).max(1.6 * head) } else { total };
        let fits = |s: f64| s - total / 2.0 >= from - 1e-9 && s + total / 2.0 <= to + 1e-9;
        // Only a head makes way for a label on the line; a shaft lies under
        // it, and the label is printed over the shaft.
        let on_label = label.filter(|l| l.on_line && style == "head");
        let s = match (number(&attrs, "arrow-pos"), on_label) {
            (Some(f), _) => Some(from + f * (to - from)),
            (None, Some(label)) => {
                // Past the label in the arrow's direction, or before it.
                let at = self.label_arc(&attrs, line);
                let clear = label.size.0 / 2.0 + 0.2 * em + total / 2.0;
                [at + sense * clear, at - sense * clear].into_iter().find(|&s| fits(s))
            }
            (None, None) => Some((from + to) / 2.0),
        };
        let Some(s) = s.filter(|&s| fits(s)) else {
            let i = self.net.index(line.index);
            let n = index_display(&i.name, i.prime);
            self.warnings.push(format!("{n}: the arrow does not fit on the line"));
            return None;
        };

        // Beside the line: the side whose normal points up (or left), or
        // away from a label beside the line.
        let beside = style == "beside";
        let offset = if beside {
            let (_, t) = line.centerline.at(s);
            let mut side = left(t);
            if side.y < -1e-9 || (side.y.abs() <= 1e-9 && side.x > 0.0) {
                side = -side;
            }
            if let Some(Anchor::Toward(dir)) = label.filter(|l| !l.on_line).map(|l| &l.anchor) {
                side = *dir;
            }
            side * (d / 2.0 + 0.3 * em + wide / 2.0)
        } else {
            V3::ZERO
        };
        let shape = flat_arrow(&line.centerline, s, sense, total, head, wide, shaft, offset);
        Some(ArrowGeom::Flat { shape, beside })
    }

    /// A cone arrow (section 8.11): the tube ends in a cone whose tip is at
    /// the end of the visible part it points to.
    fn cone(&mut self, line: &LineGeom, sense: f64, k: f64, from: f64, to: f64) -> Option<ArrowGeom> {
        let d = line.width;
        let (length, radius) = (1.7 * d * k, 0.9 * d * k);
        debug_assert!(
            self.cone_length(&self.line_attrs(line), line).is_some_and(|(_, l)| (l - length).abs() < 1e-12)
        );
        if to - from < length + d {
            let i = self.net.index(line.index);
            let n = index_display(&i.name, i.prime);
            self.warnings.push(format!("{n}: the arrow does not fit on the line"));
            return None;
        }
        let tip_at = if sense > 0.0 { to } else { from };
        let base_at = tip_at - sense * length;
        let tip = line.centerline.at(tip_at).0;
        let base = line.centerline.at(base_at).0;
        let axis = (tip - base).unit()?;
        let n = left(axis);
        let corners = [base + n * radius, tip, base - n * radius];
        let pieces = (0..3).map(|k| Piece::Line { a: corners[k], b: corners[(k + 1) % 3] }).collect();
        Some(ArrowGeom::Cone {
            outline: Path { pieces, closed: true },
            base,
            axis,
            length,
            radius,
            base_at,
            forward: sense > 0.0,
        })
    }

    fn line_label(&mut self, k: usize, line: &LineGeom) -> Option<LabelGeom> {
        let index = self.net.index(line.index);
        let (attrs, prefix) = match line.kind {
            LineKind::Bond => (self.net.bond_style(line.index), "b"),
            LineKind::Leg => {
                let (t, s) = index.holders()[0];
                (self.net.leg_style(t, s), "l")
            }
        };
        let dim = index.dim.map_or("?".into(), |d| d.to_string());
        let text = match get(&attrs, "label")? {
            Value::Math(s) => LabelText::Math(s.clone()),
            Value::Str(s) => LabelText::Plain(s.clone()),
            Value::Word(w) if w == "dim" => LabelText::Plain(dim),
            Value::Word(w) if w == "name" => {
                LabelText::Math(math_name(&index.name.base, &index.name.subscripts, index.prime))
            }
            Value::Word(_) if index.prime == 0 => LabelText::Plain(dim),
            Value::Word(_) => LabelText::Math(format!("${dim}{}$", "'".repeat(index.prime as usize))),
            _ => return None,
        };
        let id = format!("{prefix}:{}", index_display(&index.name, index.prime));
        let (size, measured) = self.label_size(&id, text.as_str());

        let s = self.label_arc(&attrs, line);
        let (mut p, tangent) = line.centerline.at(s.clamp(0.0, line.centerline.length()));
        p = p + left(tangent) * self.length(&attrs, "label-offset", false).unwrap_or(0.0);
        // A tube label goes on the tube when it fits, and beside otherwise.
        let fits = line.style == LineStyle::Tube && size.1 + size.2 <= line.width;
        let on = match word(&attrs, "label-placement") {
            Some("on") => true,
            Some(_) => false,
            None => fits,
        };
        let (pos, angle, anchor) = if on {
            if line.style == LineStyle::Tube && !fits {
                self.warnings.push(format!("{id}: the label is taller than its tube"));
            }
            let mut angle = tangent.y.atan2(tangent.x).to_degrees();
            if angle > 90.0 + 1e-9 {
                angle -= 180.0;
            } else if angle <= -90.0 + 1e-9 {
                angle += 180.0;
            }
            (p, angle, Anchor::Center)
        } else {
            let mut n = left(tangent);
            match word(&attrs, "label-side") {
                Some("left") => {}
                Some("right") => n = -n,
                _ => {
                    if n.y < -1e-9 || (n.y.abs() <= 1e-9 && n.x > 0.0) {
                        n = -n;
                    }
                }
            }
            let gap = self.length(&attrs, "label-distance", true).unwrap_or(0.15 * self.em());
            (p + n * (line.width / 2.0 + gap), 0.0, Anchor::Toward(-n))
        };
        let shift = registry::points(&attrs, "label-shift").map_or(V3::ZERO, |p| V3::xy(p[0][0], p[0][1]));
        Some(LabelGeom {
            id,
            owner: LabelOwner::Line(k),
            on_line: on,
            text,
            pos: pos + shift,
            angle,
            anchor,
            size,
            measured,
        })
    }
}
