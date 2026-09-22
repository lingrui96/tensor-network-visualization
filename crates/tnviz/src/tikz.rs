//! The TikZ backend: a figure in the runtime protocol of docs/protocol.md,
//! section 3.  It only serialises.  Positions come from geometry, colours
//! and shadings from the lighting model, and the order from the order
//! stage.

use std::fmt::Write;

use crate::geometry::{
    Anchor, ArrowGeom, Cap, Geometry, GeometryOptions, LabelOwner, LabelText, LineKind, Path, Piece,
    tube_region, tube_sides,
};
use crate::layout::V3;
use crate::lighting::{ArrowLook, Colour, LabelColour, Lighting, LineLook, Other, Shading, Stroke};
use crate::model::Network;
use crate::order::{Fragment, Part};
use crate::registry::get;
use crate::value::Value;

/// The protocol version written by `\tnvRuntime`.
pub const PROTOCOL: u32 = 3;

/// A figure as runtime-protocol TeX code, to be input inside its
/// `tikzpicture`.  `fragments` must come from [`crate::order`] and
/// `lighting` from [`crate::lighting`], both for `geom`.  `source` is the
/// tnv file's content, fingerprinted so that LaTeX can tell when the file
/// is out of date.
pub fn tikz(
    net: &Network,
    geom: &Geometry,
    lighting: &Lighting,
    fragments: &[Fragment],
    opts: &GeometryOptions,
    source: &str,
) -> String {
    let w = Writer { net, geom, lighting, unit_pt: opts.unit_pt };
    let mut s = String::new();
    // Writing to a String cannot fail.
    let _ = writeln!(s, "\\tnvRuntime{{{PROTOCOL}}}%");
    // Exact decimals: LaTeX compares them with the unit and em it has now.
    let _ = writeln!(
        s,
        "\\tnvSource{{{}}}{{{}}}{{{}}}%",
        crate::md5::md5_hex(source.as_bytes()),
        opts.unit_pt,
        opts.em_pt
    );
    for (k, expr) in lighting.slots.iter().enumerate() {
        let _ = writeln!(s, "\\tnvColor{{{k}}}{{{expr}}}%");
    }
    // Consecutive fragments of one object that is not opaque form one
    // transparency group, so that its parts fade together.
    let mut k = 0;
    while k < fragments.len() {
        let (object, opacity) = w.object(fragments[k].part);
        let mut end = k + 1;
        while end < fragments.len() && w.object(fragments[end].part).0 == object {
            end += 1;
        }
        let mut body = String::new();
        for f in &fragments[k..end] {
            w.fragment(&mut body, f.part);
        }
        if opacity < 1.0 - 1e-9 {
            let _ = write!(s, "\\tnvGroup{{{}}}{{%\n{body}}}%\n", num(opacity));
        } else {
            s += &body;
        }
        k = end;
    }
    s
}

/// What a fragment belongs to, for grouping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Object {
    Shadow(crate::model::TensorId),
    Tensor(crate::model::TensorId),
    Line(usize),
    Plane(usize),
}

struct Writer<'a> {
    net: &'a Network,
    geom: &'a Geometry,
    lighting: &'a Lighting,
    unit_pt: f64,
}

impl Writer<'_> {
    /// The object of a fragment and its opacity.  A shadow's opacity is in
    /// its fill already.
    fn object(&self, part: Part) -> (Object, f64) {
        let tensor = |t| (Object::Tensor(t), self.lighting.tensors[self.tensor_index(t)].opacity);
        let line = |k: usize| (Object::Line(k), self.lighting.line_opacity[k]);
        match part {
            Part::TensorShadow(t) => (Object::Shadow(t), 1.0),
            Part::Tensor(t) => tensor(t),
            Part::Line(k) | Part::Arrow(k) | Part::Span(k, _) => line(k),
            // A plane's fill and edge have their own opacities.
            Part::Plane(k) => (Object::Plane(k), 1.0),
            Part::Label(k) => match self.geom.labels[k].owner {
                LabelOwner::Tensor(t) => tensor(t),
                LabelOwner::Line(l) => line(l),
            },
        }
    }

    fn fragment(&self, s: &mut String, part: Part) {
        match part {
            Part::TensorShadow(t) => {
                let k = self.tensor_index(t);
                if let Some(shadow) = &self.lighting.tensors[k].shadow {
                    let path = self.geom.tensors[k].outline.transformed(0.0, shadow.offset);
                    let _ = writeln!(
                        s,
                        "\\tnvFill{{{}}}{{{}}}{{{}}}%",
                        path_code(&path),
                        colour(&shadow.colour),
                        num(shadow.opacity)
                    );
                }
            }
            Part::Tensor(t) => {
                let k = self.tensor_index(t);
                let (geom, look) = (&self.geom.tensors[k], &self.lighting.tensors[k]);
                let path = path_code(&geom.outline);
                shade(s, &path, &look.face);
                for (region, shading) in &look.patches {
                    shade(s, &path_code(region), shading);
                }
                self.stroke(s, &path, &look.outline, "round");
            }
            Part::Line(k) => {
                let line = &self.geom.lines[k];
                match &self.lighting.lines[k] {
                    LineLook::Stroke(stroke) => {
                        // A line starts inside its tensor, so only the end
                        // cap can show.
                        let cap = match line.caps.1 {
                            Cap::Round => "round",
                            Cap::Flat => "butt",
                        };
                        self.stroke(s, &path_code(&line.centerline), stroke, cap);
                    }
                    LineLook::Tube { shading, outline } => {
                        let path = path_code(line.outline.as_ref().expect("a tube has an outline"));
                        shade(s, &path, shading);
                        self.stroke(s, &path, outline, "round");
                    }
                }
            }
            Part::Arrow(k) => match (&self.geom.lines[k].arrow, &self.lighting.arrows[k]) {
                (Some(ArrowGeom::Flat { shape, .. }), Some(ArrowLook::Fill(fill))) => {
                    let _ = writeln!(s, "\\tnvFill{{{}}}{{{}}}{{1}}%", path_code(shape), colour(fill));
                }
                (
                    Some(ArrowGeom::Cone { outline, .. }),
                    Some(ArrowLook::Cone { shading, outline: stroke }),
                ) => {
                    let path = path_code(outline);
                    shade(s, &path, shading);
                    self.stroke(s, &path, stroke, "round");
                }
                _ => {}
            },
            Part::Span(k, i) => self.span(s, k, i),
            Part::Plane(k) => {
                let (geom, look) = (&self.geom.planes[k], &self.lighting.planes[k]);
                let path = path_code(&geom.outline);
                let _ =
                    writeln!(s, "\\tnvFill{{{path}}}{{{}}}{{{}}}%", colour(&look.fill), num(look.opacity));
                let mut edge = String::new();
                self.stroke(&mut edge, &path, &look.edge, "round");
                let _ = write!(s, "\\tnvGroup{{{}}}{{%\n{edge}}}%\n", num(look.edge_opacity));
            }
            Part::Label(k) => self.label(s, k),
        }
    }

    /// One span of a 3D line: its part of the stroke, or of the tube, with
    /// the tube's sides stroked, and its caps only at the line's own ends.
    fn span(&self, s: &mut String, k: usize, i: usize) {
        let line = &self.geom.lines[k];
        let span = line.spans[i];
        let total = line.centerline.length();
        // A cone ends the tube at its base.
        let (lo, hi) = match &line.arrow {
            Some(ArrowGeom::Cone { base_at, forward: true, .. }) => (0.0, *base_at),
            Some(ArrowGeom::Cone { base_at, forward: false, .. }) => (*base_at, total),
            _ => (0.0, total),
        };
        let (from, to) = (span.from.max(lo), span.to.min(hi));
        if to <= from + 1e-9 {
            return;
        }
        let piece = line.centerline.slice(from, to);
        match &self.lighting.lines[k] {
            LineLook::Stroke(stroke) => self.stroke(s, &path_code(&piece), stroke, "round"),
            LineLook::Tube { shading, outline } => {
                let r = line.width / 2.0;
                // Round caps only at the line's own ends, not at a cone.
                let own =
                    |at: f64, end: f64, kind: Cap| if (at - end).abs() < 1e-9 { kind } else { Cap::Flat };
                let caps = (own(from, 0.0, line.caps.0), own(to, total, line.caps.1));
                // Where the tube meets its tensors, at the line's own ends.
                let at_start = (from - 0.0).abs() < 1e-9;
                let at_end = (to - total).abs() < 1e-9;
                let junctions = [
                    line.junctions[0].as_deref().filter(|_| at_start),
                    line.junctions[1].as_deref().filter(|_| at_end),
                ];
                shade(s, &path_code(&tube_region(&piece, r, caps, junctions)), shading);
                for side in tube_sides(&piece, r) {
                    self.stroke(s, &path_code(&side), outline, "round");
                }
                for curve in junctions.into_iter().flatten() {
                    let pieces = curve.windows(2).map(|w| Piece::Line { a: w[0], b: w[1] }).collect();
                    self.stroke(s, &path_code(&Path { pieces, closed: false }), outline, "round");
                }
                let arc = |p: V3, d: V3| Path {
                    pieces: vec![Piece::Arc {
                        center: p,
                        radius: r,
                        start: d.x.atan2(-d.y),
                        sweep: -std::f64::consts::PI,
                    }],
                    closed: false,
                };
                if caps.0 == Cap::Round && junctions[0].is_none() {
                    let (p, d) = piece.at(0.0);
                    self.stroke(s, &path_code(&arc(p, -d)), outline, "round");
                }
                if caps.1 == Cap::Round && junctions[1].is_none() {
                    let (p, d) = piece.at(piece.length());
                    self.stroke(s, &path_code(&arc(p, d)), outline, "round");
                }
            }
        }
    }

    fn tensor_index(&self, t: crate::model::TensorId) -> usize {
        self.geom.tensors.iter().position(|g| g.tensor == t).expect("a fragment of a known tensor")
    }

    fn stroke(&self, s: &mut String, path: &str, stroke: &Stroke, cap: &str) {
        let _ = writeln!(
            s,
            "\\tnvStroke{{{path}}}{{{}}}{{{}pt}}{{{cap}}}%",
            colour(&stroke.colour),
            num(stroke.width * self.unit_pt)
        );
    }

    fn label(&self, s: &mut String, k: usize) {
        let label = &self.geom.labels[k];
        let attrs = match label.owner {
            LabelOwner::Tensor(t) => self.net.tensor_style(t),
            LabelOwner::Line(l) => {
                let line = &self.geom.lines[l];
                match line.kind {
                    LineKind::Bond => self.net.bond_style(line.index),
                    LineKind::Leg => {
                        let (t, slot) = self.net.index(line.index).holders()[0];
                        self.net.leg_style(t, slot)
                    }
                }
            }
        };
        let font = match get(&attrs, "label-font") {
            Some(Value::Word(f) | Value::Str(f) | Value::Math(f)) => f.clone(),
            _ => String::new(),
        };
        let anchor = match &label.anchor {
            Anchor::Center => "center".to_string(),
            Anchor::Toward(d) => num(d.y.atan2(d.x).to_degrees()),
        };
        let colour = match &self.lighting.labels[k] {
            LabelColour::Fixed(c) => colour(c),
            LabelColour::Auto { background, weights, threshold, dark, light } => format!(
                "\\tnvAutoColor{{{background}}}{{{}}}{{{}}}{{{}}}{{{}}}{{{}}}{{{}}}",
                num(weights[0]),
                num(weights[1]),
                num(weights[2]),
                num(*threshold),
                colour(dark),
                colour(light)
            ),
        };
        let (w, h, d) = label.size;
        let pt = |x: f64| format!("{:.3}", x * self.unit_pt);
        let text = match &label.text {
            LabelText::Math(m) => m.clone(),
            LabelText::Plain(p) => escape(p),
        };
        let _ = writeln!(
            s,
            "\\tnvText{{{}}}{{{}}}{{{}}}{{{}}}{{{anchor}}}{{{font}}}{{{colour}}}{{{}}}{{{}}}{{{}}}{{{text}}}%",
            label.id,
            num(label.pos.x),
            num(label.pos.y),
            num(label.angle),
            pt(w),
            pt(h),
            pt(d)
        );
    }
}

fn shade(s: &mut String, path: &str, shading: &Shading) {
    let code = shading.program.to_postscript(&|slot, ch| {
        let c = ["R", "G", "B"][ch as usize];
        format!("\\tnv{c}{{{slot}}}")
    });
    let _ = writeln!(
        s,
        "\\tnvShade{{{path}}}{{{}}}{{{}}}{{{}}}{{{code}}}%",
        num(shading.center.x),
        num(shading.center.y),
        num(shading.extent)
    );
}

/// A colour argument: `\tnvSlot{k}`, or a mix such as `\tnvSlot{k}!76!black`.
fn colour(c: &Colour) -> String {
    let percent = c.keep * 100.0;
    if (percent - 100.0).abs() < 1e-9 {
        return format!("\\tnvSlot{{{}}}", c.slot);
    }
    let other = match c.other {
        Other::White => "white",
        Other::Black => "black",
    };
    format!("\\tnvSlot{{{}}}!{}!{other}", c.slot, num(percent))
}

/// A path in TikZ syntax, in layout units.
fn path_code(path: &Path) -> String {
    let mut s = String::new();
    let mut at = None;
    for piece in &path.pieces {
        let start = piece.start();
        match at {
            None => s += &point(start),
            Some(p) if (start - p).norm() > 1e-6 => s += &format!(" -- {}", point(start)),
            Some(_) => {}
        }
        match *piece {
            Piece::Line { b, .. } => s += &format!(" -- {}", point(b)),
            // A fillet shrunk to a point is only a corner.
            Piece::Arc { radius, .. } if radius < 1e-6 => {}
            Piece::Arc { radius, start, sweep, .. } => {
                s += &format!(
                    " arc[start angle={}, delta angle={}, radius={}]",
                    num(start.to_degrees()),
                    num(sweep.to_degrees()),
                    num(radius)
                )
            }
        }
        at = Some(piece.end());
    }
    if path.closed {
        s += " -- cycle";
    }
    s
}

fn point(p: crate::layout::V3) -> String {
    format!("({},{})", num(p.x), num(p.y))
}

/// A number for TeX: at most 4 decimals, no exponent, no `-0`.
fn num(x: f64) -> String {
    let s = format!("{x:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// Plain text with TeX's special characters escaped.
fn escape(text: &str) -> String {
    let mut s = String::new();
    for c in text.chars() {
        match c {
            '\\' => s += "\\textbackslash{}",
            '~' => s += "\\textasciitilde{}",
            '^' => s += "\\textasciicircum{}",
            '{' | '}' | '$' | '&' | '#' | '_' | '%' => {
                s.push('\\');
                s.push(c);
            }
            _ => s.push(c),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LabelSizes, geometry, layout, lighting, order, parse};

    fn figure(src: &str) -> String {
        let net = parse(src).unwrap();
        let lay = layout(&net).unwrap();
        let opts = GeometryOptions::default();
        let geom = geometry(&net, &lay, &opts, &LabelSizes::new()).unwrap();
        let light = lighting(&net, &geom, &opts);
        tikz(&net, &geom, &light, &order(&net, &geom), &opts, src)
    }

    #[test]
    fn a_figure_in_protocol_order() {
        let s = figure("A at (0, 0)\nB at (3, 0)\nA - B [style=tube, label=\"k_1 & 50%\"]\n");
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "\\tnvRuntime{3}%");
        let src = "A at (0, 0)\nB at (3, 0)\nA - B [style=tube, label=\"k_1 & 50%\"]\n";
        assert_eq!(
            lines[1],
            format!("\\tnvSource{{{}}}{{28.45274}}{{10}}%", crate::md5::md5_hex(src.as_bytes()))
        );
        assert!(lines[2].starts_with("\\tnvColor{0}{black!48}"));
        let first = |cmd: &str| lines.iter().position(|l| l.starts_with(cmd)).unwrap();
        // The tube, then the shadows, the tensors, and the labels.
        assert!(first("\\tnvShade") < first("\\tnvFill"));
        assert!(first("\\tnvFill") < first("\\tnvText"));
        assert!(s.contains("{$A$}%") && s.contains("{$B$}%"));
        assert!(s.contains("{k\\_1 \\& 50\\%}%"));
        assert!(s.contains("\\tnvSlot{0}!76!black"));
        // Every brace is balanced on every line.
        for l in &lines {
            let depth = l.chars().fold(0i32, |d, c| match c {
                '{' => d + 1,
                '}' => d - 1,
                _ => d,
            });
            let escaped = l.matches("\\{").count() as i32 - l.matches("\\}").count() as i32;
            assert_eq!(depth - escaped, 0, "{l}");
        }
    }

    #[test]
    fn paths_and_numbers() {
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(2.50), "2.5");
        assert_eq!(num(1e-9), "0");
        let s = figure("A at (0, 0)\nA [shape=orb, label=none, shadow=off]\n");
        assert!(s.contains("arc[start angle=0, delta angle=360, radius=0.4] -- cycle"), "{s}");
    }

    #[test]
    fn translucent_objects_are_groups() {
        let s = figure("A at (0, 0)\nB at (3, 0)\nA - B [opacity=.4, arrow=forward]\nA [opacity=.5]\n");
        // A's body, outline, and label in one group; its shadow faded directly.
        let group = s.split("\\tnvGroup{0.5}{%\n").nth(1).expect("a group for A");
        let group = &group[..group.find("\n}%\n").expect("the group's end")];
        assert!(group.contains("\\tnvShade") && group.contains("\\tnvStroke") && group.contains("{$A$}%"));
        assert!(s.contains("}{0.06}%"), "the shadow fades by .5: {s}");
        // The bond and its arrow together.
        let bond = s.split("\\tnvGroup{0.4}{%\n").nth(1).expect("a group for the bond");
        assert!(bond.starts_with("\\tnvStroke") && bond.contains("\\tnvFill"));
        // B is opaque: no group.
        assert_eq!(s.matches("\\tnvGroup").count(), 2);
    }

    #[test]
    fn names_are_math() {
        let s = figure("chain A[1..2]\n");
        assert!(s.contains("{$A_{1}$}%") && s.contains("{$A_{2}$}%"));
    }
}
