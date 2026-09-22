//! The TikZ backend: a figure in the runtime protocol of docs/protocol.md,
//! section 3.  It only serialises.  Positions come from geometry, colours
//! and shadings from the lighting model, and the order from the order
//! stage.

use std::fmt::Write;

use crate::geometry::{Anchor, Cap, Geometry, GeometryOptions, LabelOwner, LabelText, LineKind, Path, Piece};
use crate::lighting::{Colour, LabelColour, Lighting, LineLook, Other, Shading, Stroke};
use crate::model::Network;
use crate::order::{Fragment, Part};
use crate::registry::get;
use crate::value::Value;

/// The protocol version written by `\tnvRuntime`.
pub const PROTOCOL: u32 = 2;

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
    for f in fragments {
        w.fragment(&mut s, f.part);
    }
    s
}

struct Writer<'a> {
    net: &'a Network,
    geom: &'a Geometry,
    lighting: &'a Lighting,
    unit_pt: f64,
}

impl Writer<'_> {
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
            Part::Label(k) => self.label(s, k),
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
        assert_eq!(lines[0], "\\tnvRuntime{2}%");
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
    fn names_are_math() {
        let s = figure("chain A[1..2]\n");
        assert!(s.contains("{$A_{1}$}%") && s.contains("{$A_{2}$}%"));
    }
}
