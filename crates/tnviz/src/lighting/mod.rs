//! The lighting model (docs/lighting.md): how every surface is coloured,
//! as formulas of its base colour's RGB.
//!
//! The engine never resolves colours.  Base colours are xcolor expressions
//! collected into slots; derived colours are mixes of a slot with white or
//! black, and shadings are programs whose parameters are slot channels.  A
//! backend supplies the RGB of each slot: the TeX runtime through xcolor, a
//! raster backend through its own evaluator.

pub mod expr;

use expr::{
    Expr, Program, abs, add, and, c, clamp01, div, exp, if_, lt, max, min, mul, neg, pow, smooth, sqrt, sub,
};

use crate::geometry::{
    ArrowGeom, Geometry, GeometryOptions, LabelOwner, LineKind, LineStyle, Piece, Shape, TensorGeom,
};
use crate::geometry::{Patch, Path, View, edge_toward};
use crate::layout::V3;
use crate::model::{Dim, Network};
use crate::registry::{get, number, word};
use crate::value::{Attr, Value};

/// A colour: a base colour mixed with white or black, as xcolor's
/// `<slot>!<keep·100>!<other>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colour {
    pub slot: usize,
    /// The fraction of the base colour, in [0, 1].
    pub keep: f64,
    pub other: Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Other {
    White,
    Black,
}

impl Colour {
    pub fn base(slot: usize) -> Self {
        Colour { slot, keep: 1.0, other: Other::White }
    }

    fn mix(slot: usize, keep: f64, other: Other) -> Self {
        Colour { slot, keep, other }
    }

    /// The colour's RGB, given the base colours'.
    pub fn eval(&self, rgb: &dyn Fn(usize) -> [f64; 3]) -> [f64; 3] {
        let base = rgb(self.slot);
        let o = if self.other == Other::White { 1.0 } else { 0.0 };
        base.map(|x| self.keep * x + (1.0 - self.keep) * o)
    }
}

/// A shading: a program over the square of half-size `extent` centred at
/// `center`, in page coordinates and layout units.
#[derive(Clone, Debug, PartialEq)]
pub struct Shading {
    pub center: V3,
    pub extent: f64,
    pub program: Program,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub colour: Colour,
    /// In layout units.
    pub width: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub offset: V3,
    pub colour: Colour,
    pub opacity: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TensorLook {
    pub face: Shading,
    /// In 3D: the visible surface of a rounded solid, drawn over `face` in
    /// order, each shading clipped to its region.
    pub patches: Vec<(Path, Shading)>,
    pub outline: Stroke,
    pub shadow: Option<Shadow>,
    /// The tensor's opacity, for its body, outline, and label together.
    pub opacity: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LineLook {
    Stroke(Stroke),
    Tube { shading: Shading, outline: Stroke },
}

/// A label's text colour.
#[derive(Clone, Debug, PartialEq)]
pub enum LabelColour {
    Fixed(Colour),
    /// `dark` when wr·R + wg·G + wb·B of the background slot exceeds
    /// `threshold`, `light` otherwise.
    Auto {
        background: usize,
        weights: [f64; 3],
        threshold: f64,
        dark: Colour,
        light: Colour,
    },
}

/// A plane (language section 11.6): a flat translucent fill and its edge.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaneLook {
    pub fill: Colour,
    pub opacity: f64,
    pub edge: Stroke,
    pub edge_opacity: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArrowLook {
    Fill(Colour),
    /// A cone: its shading and the stroke of its edges.
    Cone {
        shading: Shading,
        outline: Stroke,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lighting {
    /// Base colours as xcolor expressions, referred to by slot.
    pub slots: Vec<String>,
    /// Indexed like `Geometry::tensors`.
    pub tensors: Vec<TensorLook>,
    /// Indexed like `Geometry::lines`.
    pub lines: Vec<LineLook>,
    /// Each line's arrow, indexed like `Geometry::lines`.
    pub arrows: Vec<Option<ArrowLook>>,
    /// Each line's opacity, for the line, its arrow, and its label
    /// together; indexed like `Geometry::lines`.
    pub line_opacity: Vec<f64>,
    /// Indexed like `Geometry::planes`.
    pub planes: Vec<PlaneLook>,
    /// Indexed like `Geometry::labels`.
    pub labels: Vec<LabelColour>,
}

// ---- Constants of the model ----------------------------------------------
//
// Lengths are layout units, tuned at 1 unit = 1cm; the prototype's point
// sizes are converted at that scale.

const DEFAULT_LIGHT: f64 = 135.0;
const TENSOR_COLOUR: &str = "black!48";
const LINE_COLOUR: &str = "black!72";
const TUBE_COLOUR: &str = "black!42";
const PLANE_COLOUR: &str = "black!35";
const PLANE_OPACITY: f64 = 0.18;
const SHADOW_OFFSET: f64 = 0.1;
const SHADOW_OPACITY: f64 = 0.12;
const TENSOR_OUTLINE_EM: f64 = 0.055;
const TUBE_OUTLINE_EM: f64 = 0.04;
/// Rim width at highlight size .22: 1.6pt plus .18 of the corner radius.
const RIM_BASE: f64 = 0.0562;
const RIM_PER_RADIUS: f64 = 0.18;
const SPECULAR_PEAK: f64 = 0.88;
const BACK_PEAK: f64 = 0.46;
const BACK_WIDTH: f64 = 1.8;
/// Fan width of the blended normals inside a corner, per unit of depth.
const FAN: f64 = 2.0;
/// Below this length the blended normal fades out: two edges blended
/// equally stay above it (0.5 for a triangle), the cancelling normals
/// deep inside fall below it.
const FADE: f64 = 0.4;
/// Orbs: how far the light wraps past the terminator, the core shadow's
/// tone, the glint, and the reflected light's width (as the normal's height
/// above the page) and strength.
const ORB_ELEVATION: f64 = 20.0;
const ORB_LIT: f64 = 0.70;
const ORB_WRAP: f64 = 0.2;
const ORB_CORE: f64 = 0.66;
const ORB_GLINT_PEAK: f64 = 0.85;
const ORB_SHININESS: f64 = 40.0;
const ORB_BOUNCE: f64 = 0.3;
const ORB_BOUNCE_PEAK: f64 = 0.6;
/// Tube light: elevation above the page and the Blinn-Phong terms.
const TUBE_ELEVATION: f64 = 38.0;
const TUBE_AMBIENT: f64 = 0.40;
const TUBE_DIFFUSE: f64 = 0.72;
const TUBE_SPECULAR: f64 = 0.62;
const TUBE_SHININESS: f64 = 26.0;
const LUMINANCE: [f64; 3] = [0.2126, 0.7152, 0.0722];
/// A squared distance beyond any figure.  PDF readers take numbers written
/// without a point as integers, which must stay below 2³¹.
const FAR: f64 = 1e6;

struct Slots(Vec<String>);

impl Slots {
    fn get(&mut self, expr: &str) -> usize {
        match self.0.iter().position(|s| s == expr) {
            Some(k) => k,
            None => {
                self.0.push(expr.to_string());
                self.0.len() - 1
            }
        }
    }

    fn colour_attr(&mut self, attrs: &[Attr], key: &str, default: &str) -> usize {
        let expr = match get(attrs, key) {
            Some(Value::Word(w) | Value::Str(w)) => w.clone(),
            _ => default.to_string(),
        };
        self.get(&expr)
    }
}

/// Light the geometry of a network.
pub fn lighting(net: &Network, geom: &Geometry, opts: &GeometryOptions) -> Lighting {
    let em = opts.em_pt / opts.unit_pt;
    let three = net.scene().dim == Dim::Three;
    let light3 = light_vector(net);
    // In 3D the page angle of the light, for shadows, comes from its vector.
    let light = if three {
        light3.y.atan2(light3.x).to_degrees()
    } else {
        net.scene().light.as_ref().and_then(|v| v.first().copied()).unwrap_or(DEFAULT_LIGHT)
    };
    let mut slots = Slots(Vec::new());

    let mut tensor_slots = Vec::new();
    let tensors = geom
        .tensors
        .iter()
        .map(|t| {
            let attrs = net.tensor_style(t.tensor);
            let slot = slots.colour_attr(&attrs, "color", TENSOR_COLOUR);
            tensor_slots.push(slot);
            let hl = number(&attrs, "highlight-size").unwrap_or(0.66) / 0.22;
            let inset = length(&attrs, "highlight-inset").unwrap_or(0.0);
            let (face, patches) = match (t.shape, t.patches.as_slice()) {
                (_, [Patch::Sphere { center, radius }]) if three => {
                    (sphere3(*center, *radius, slot, light3, true, &t.outline), Vec::new())
                }
                (_, patches) if three => (
                    flat(slot, &t.outline),
                    patches.iter().map(|p| (p.region(), patch_shading(p, slot, light3))).collect(),
                ),
                (Shape::Orb | Shape::Dot, _) => (orb(t, slot, light), Vec::new()),
                _ => (glass_face(t, slot, light, hl, inset), Vec::new()),
            };
            let opacity = number(&attrs, "opacity").unwrap_or(1.0);
            // A shadow is one fill: it fades by the same factor directly.
            let shadow = (word(&attrs, "shadow") != Some("off")).then(|| Shadow {
                offset: V3::xy(-light.to_radians().cos(), -light.to_radians().sin()) * SHADOW_OFFSET,
                colour: Colour::mix(slot, 0.6, Other::Black),
                opacity: SHADOW_OPACITY * opacity,
            });
            TensorLook {
                face,
                patches,
                outline: Stroke {
                    colour: Colour::mix(slot, 0.76, Other::Black),
                    width: TENSOR_OUTLINE_EM * em,
                },
                shadow,
                opacity,
            }
        })
        .collect();

    let mut line_slots = Vec::new();
    let lines = geom
        .lines
        .iter()
        .map(|l| {
            let attrs = line_attrs(net, l.kind, l.index);
            let tube = l.style == LineStyle::Tube;
            let slot = slots.colour_attr(&attrs, "color", if tube { TUBE_COLOUR } else { LINE_COLOUR });
            line_slots.push(slot);
            match (&l.outline, tube) {
                (Some(outline), true) => LineLook::Tube {
                    shading: if three {
                        tube_shading3(
                            &l.centerline.pieces,
                            &l.axes,
                            outline.sample(0.2),
                            l.width / 2.0,
                            slot,
                            light3,
                        )
                    } else {
                        tube_shading(&l.centerline.pieces, outline.sample(0.2), l.width / 2.0, slot, light)
                    },
                    outline: Stroke {
                        colour: Colour::mix(slot, 0.62, Other::Black),
                        width: TUBE_OUTLINE_EM * em,
                    },
                },
                _ => LineLook::Stroke(Stroke { colour: Colour::base(slot), width: l.width }),
            }
        })
        .collect();

    // Flat arrows take the line's colour; on a tube, the glint's; beside a
    // tube, its outline's.  A cone is lit like the tube it grows from.
    let arrows = geom
        .lines
        .iter()
        .zip(&line_slots)
        .map(|(l, &slot)| {
            let outline = Colour::mix(slot, 0.62, Other::Black);
            l.arrow.as_ref().map(|a| match (a, l.style) {
                (ArrowGeom::Flat { beside: false, .. }, LineStyle::Tube) => {
                    ArrowLook::Fill(Colour::mix(slot, 0.06, Other::White))
                }
                (ArrowGeom::Flat { beside: true, .. }, LineStyle::Tube) => ArrowLook::Fill(outline),
                (ArrowGeom::Flat { .. }, LineStyle::Line) => ArrowLook::Fill(Colour::base(slot)),
                (cone @ ArrowGeom::Cone { .. }, _) => ArrowLook::Cone {
                    shading: cone_shading(cone, slot, light),
                    outline: Stroke { colour: outline, width: TUBE_OUTLINE_EM * em },
                },
            })
        })
        .collect();

    let black = slots.get("black");
    let light_text = slots.get("white!92!black");
    let labels = geom
        .labels
        .iter()
        .map(|label| {
            let attrs = match label.owner {
                LabelOwner::Tensor(t) => net.tensor_style(t),
                LabelOwner::Line(k) => line_attrs(net, geom.lines[k].kind, geom.lines[k].index),
            };
            if let Some(Value::Word(w) | Value::Str(w)) = get(&attrs, "label-color")
                && w != "auto"
            {
                return LabelColour::Fixed(Colour::base(slots.get(w)));
            }
            let auto = |background: usize, factor: f64, threshold: f64| LabelColour::Auto {
                background,
                weights: LUMINANCE.map(|w| w * factor),
                threshold,
                dark: Colour::base(black),
                light: Colour::base(light_text),
            };
            match label.owner {
                LabelOwner::Tensor(t) => auto(tensor_slots[t.0], 1.0, 0.5),
                // On a tube, the text sits on its bright front.
                LabelOwner::Line(k) if label.on_line && geom.lines[k].style == LineStyle::Tube => {
                    auto(line_slots[k], 0.85, 0.45)
                }
                LabelOwner::Line(_) => LabelColour::Fixed(Colour::base(black)),
            }
        })
        .collect();

    let line_opacity = geom
        .lines
        .iter()
        .map(|l| number(&line_attrs(net, l.kind, l.index), "opacity").unwrap_or(1.0))
        .collect();
    let planes = (0..geom.planes.len())
        .map(|k| {
            let attrs = net.plane_style(k);
            let slot = slots.colour_attr(&attrs, "color", PLANE_COLOUR);
            let opacity = number(&attrs, "opacity").unwrap_or(PLANE_OPACITY);
            PlaneLook {
                fill: Colour::base(slot),
                opacity,
                edge: Stroke { colour: Colour::mix(slot, 0.7, Other::Black), width: TUBE_OUTLINE_EM * em },
                edge_opacity: (2.5 * opacity).min(1.0),
            }
        })
        .collect();
    Lighting { slots: slots.0, tensors, lines, arrows, line_opacity, planes, labels }
}

fn line_attrs(net: &Network, kind: LineKind, index: crate::model::IndexId) -> Vec<Attr> {
    match kind {
        LineKind::Bond => net.bond_style(index),
        LineKind::Leg => {
            let (t, s) = net.index(index).holders()[0];
            net.leg_style(t, s)
        }
    }
}

fn length(attrs: &[Attr], key: &str) -> Option<f64> {
    match get(attrs, key)? {
        Value::Number(x) => Some(*x),
        _ => None,
    }
}

fn param(slot: usize, channel: u8) -> Expr {
    Expr::Param { slot, channel }
}

/// `keep·c + (1 − keep)·other` for one channel.
fn mixed(slot: usize, channel: u8, keep: f64, white: bool) -> Expr {
    let p = mul(c(keep), param(slot, channel));
    if white { add(p, c(1.0 - keep)) } else { p }
}

fn dir(angle: f64) -> V3 {
    V3::xy(angle.to_radians().cos(), angle.to_radians().sin())
}

/// A shading domain around a set of points.
fn domain(points: &[V3]) -> (V3, f64) {
    let (mut lo, mut hi) =
        (V3::xy(f64::INFINITY, f64::INFINITY), V3::xy(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in points {
        lo = V3::xy(lo.x.min(p.x), lo.y.min(p.y));
        hi = V3::xy(hi.x.max(p.x), hi.y.max(p.y));
    }
    let center = (lo + hi) * 0.5;
    (center, ((hi.x - lo.x).max(hi.y - lo.y)) / 2.0 + 0.05)
}

// ---- Glass faces -----------------------------------------------------------

/// A polygonal tensor.  For every point the program finds the nearest
/// point of the rounded outline: inside a fillet's wedge it lies radially
/// from the fillet's centre, elsewhere it comes from the lines of the
/// fillet-centre polygon, whose normals and distances are blended smoothly
/// across the bisectors.  That gives a depth below the outline and a normal;
/// the normal's facing towards the light makes a specular rim or a darker
/// back rim, over a weak linear gradient along the light.
fn glass_face(t: &TensorGeom, slot: usize, light: f64, hl: f64, inset: f64) -> Shading {
    let corners: Vec<V3> = t
        .shape
        .corners(t.width, t.height)
        .expect("polygonal shape")
        .into_iter()
        .map(|p| t.center + p.rotate_z(t.rotation))
        .collect();
    let n = corners.len();
    let r = t.corner_radius;
    let edge = |k: usize| (corners[(k + 1) % n] - corners[k]).unit().unwrap();
    let normal = |k: usize| {
        let e = edge(k);
        V3::xy(e.y, -e.x)
    };
    let (x, y) = (Expr::X, Expr::Y);
    let mut p = Program::new();

    // Signed distances to the lines of the fillet-centre polygon.
    let s: Vec<Expr> = (0..n)
        .map(|k| {
            let nk = normal(k);
            let offset = nk.x * corners[k].x + nk.y * corners[k].y - r;
            p.bind(sub(add(mul(c(nk.x), x.clone()), mul(c(nk.y), y.clone())), c(offset)))
        })
        .collect();
    let smax = p.bind(s.iter().cloned().reduce(max).unwrap());
    let beta = p.bind(add(mul(c(FAN), max(c(0.0), neg(smax.clone()))), c(1e-4)));
    let w: Vec<Expr> =
        s.iter().map(|sk| p.bind(exp(div(sub(sk.clone(), smax.clone()), beta.clone())))).collect();
    let sum = |p: &mut Program, f: &dyn Fn(usize) -> Expr| {
        let e = (0..n).map(f).reduce(add).unwrap();
        p.bind(e)
    };
    let wsum = sum(&mut p, &|k| w[k].clone());
    let nx = sum(&mut p, &|k| mul(w[k].clone(), c(normal(k).x)));
    let ny = sum(&mut p, &|k| mul(w[k].clone(), c(normal(k).y)));
    let ssum = sum(&mut p, &|k| mul(w[k].clone(), s[k].clone()));
    // The facing of the blended normal.  Deep inside, where the normals of
    // many edges cancel and its direction is undefined, it fades to 0.
    let l = dir(light);
    let len = p.bind(sqrt(add(mul(nx.clone(), nx.clone()), mul(ny.clone(), ny.clone()))));
    let spread = p.bind(clamp01(div(len.clone(), mul(c(FADE), wsum.clone()))));
    let mut facing = mul(div(add(mul(nx, c(l.x)), mul(ny, c(l.y))), add(len, c(1e-9))), smooth(spread));
    let mut depth = sub(c(r), div(ssum, wsum));

    // Fillet wedges override the blend.
    if r > 1e-9 {
        for i in (0..n).rev() {
            let h = (i + n - 1) % n;
            let (nh, ni) = (normal(h), normal(i));
            let centre = corners[i] - (nh + ni) * (r / (1.0 + nh.x * ni.x + nh.y * ni.y));
            let vx = p.bind(sub(x.clone(), c(centre.x)));
            let vy = p.bind(sub(y.clone(), c(centre.y)));
            let (eh, ei) = (edge(h), edge(i));
            let inside = p.bind(and(
                lt(c(0.0), add(mul(vx.clone(), c(eh.x)), mul(vy.clone(), c(eh.y)))),
                lt(add(mul(vx.clone(), c(ei.x)), mul(vy.clone(), c(ei.y))), c(0.0)),
            ));
            let rho = p.bind(sqrt(add(mul(vx.clone(), vx.clone()), mul(vy.clone(), vy.clone()))));
            let radial = div(add(mul(vx, c(l.x)), mul(vy, c(l.y))), add(rho.clone(), c(1e-9)));
            facing = if_(inside.clone(), radial, facing);
            depth = if_(inside, sub(c(r), rho), depth);
        }
    }
    let depth = p.bind(depth);
    let facing = p.bind(facing);

    let rim = hl * (RIM_BASE + RIM_PER_RADIUS * r);
    let ts = p.bind(clamp01(sub(c(1.0), div(abs(sub(depth.clone(), c(inset))), c(rim)))));
    let spec = p.bind(mul(c(SPECULAR_PEAK), mul(smooth(ts), pow(max(c(0.0), facing.clone()), c(2.0)))));
    let tb = p.bind(clamp01(sub(c(1.0), div(depth, c(BACK_WIDTH * rim)))));
    let back = p.bind(mul(c(BACK_PEAK), mul(smooth(tb), max(c(0.0), neg(facing)))));

    // Gradient along the light, over the shape's extent in that direction.
    let reach =
        corners.iter().map(|q| (*q - t.center).x * l.x + (*q - t.center).y * l.y).fold(1e-9, f64::max);
    let g = p.bind(clamp01(add(
        c(0.5),
        mul(
            c(0.5 / reach),
            add(mul(c(l.x), sub(x.clone(), c(t.center.x))), mul(c(l.y), sub(y, c(t.center.y)))),
        ),
    )));

    let channel = |ch: u8| {
        let deep = mixed(slot, ch, 0.78, false);
        let lit = mixed(slot, ch, 0.80, true);
        let body = add(deep.clone(), mul(sub(lit, deep), g.clone()));
        let with_spec =
            add(mul(body, sub(c(1.0), spec.clone())), mul(mixed(slot, ch, 0.06, true), spec.clone()));
        add(mul(with_spec, sub(c(1.0), back.clone())), mul(mixed(slot, ch, 0.35, false), back.clone()))
    };
    p.output([channel(0), channel(1), channel(2)]);
    let (center, extent) = domain(&t.outline.sample(0.2));
    Shading { center, extent, program: p }
}

/// An orb: a sphere, lit by the world light raised above the page like a
/// tube's, in the palette of the glass faces.  From the surface normal come
/// the tones of a lit sphere: glint, lit side, the base colour as the
/// halftone, a core shadow inside the silhouette, and reflected light at
/// the shadow side's edge, which makes the sphere read as round.
fn orb(t: &TensorGeom, slot: usize, light: f64) -> Shading {
    let radius = t.width / 2.0;
    let el = ORB_ELEVATION.to_radians();
    let l = V3::new(el.cos() * light.to_radians().cos(), el.cos() * light.to_radians().sin(), el.sin());
    let h = V3::new(l.x, l.y, l.z + 1.0).unit().unwrap();
    let flat = dir(light);
    let mut p = Program::new();
    let nx = p.bind(div(sub(Expr::X, c(t.center.x)), c(radius)));
    let ny = p.bind(div(sub(Expr::Y, c(t.center.y)), c(radius)));
    let r2 = p.bind(add(mul(nx.clone(), nx.clone()), mul(ny.clone(), ny.clone())));
    let nz = p.bind(sqrt(max(c(0.0), sub(c(1.0), r2))));
    let dot = |v: V3| add(add(mul(nx.clone(), c(v.x)), mul(ny.clone(), c(v.y))), mul(nz.clone(), c(v.z)));
    // Wrapped Lambert: 0 in the core shadow, 1/2 at the halftone, 1 lit.
    let w = p.bind(clamp01(div(add(dot(l), c(ORB_WRAP)), c(1.0 + ORB_WRAP))));
    let to_base = p.bind(clamp01(mul(c(2.0), w.clone())));
    let to_lit = p.bind(clamp01(sub(mul(c(2.0), w), c(1.0))));
    let glint = p.bind(mul(c(ORB_GLINT_PEAK), pow(max(c(0.0), dot(h)), c(ORB_SHININESS))));
    // Reflected light: at the silhouette, on the side away from the light.
    let away = p.bind(max(c(0.0), neg(add(mul(nx.clone(), c(flat.x)), mul(ny.clone(), c(flat.y))))));
    let tr = p.bind(clamp01(sub(c(1.0), div(nz, c(ORB_BOUNCE)))));
    let bounce = p.bind(mul(c(ORB_BOUNCE_PEAK), mul(smooth(tr), away)));

    let channel = |ch: u8| {
        let core = mixed(slot, ch, ORB_CORE, false);
        let base = param(slot, ch);
        let lit = mixed(slot, ch, ORB_LIT, true);
        let body = add(
            add(core.clone(), mul(sub(base.clone(), core), to_base.clone())),
            mul(sub(lit, base.clone()), to_lit.clone()),
        );
        let bounced = add(mul(body, sub(c(1.0), bounce.clone())), mul(base, bounce.clone()));
        add(mul(bounced, sub(c(1.0), glint.clone())), mul(mixed(slot, ch, 0.06, true), glint.clone()))
    };
    p.output([channel(0), channel(1), channel(2)]);
    let (center, extent) = domain(&t.outline.sample(0.2));
    Shading { center, extent, program: p }
}

// ---- 3D ----------------------------------------------------------------------

/// The 3D light in view coordinates (language section 11.8), unit length.
fn light_vector(net: &Network) -> V3 {
    let scene = net.scene();
    let v = match scene.light.as_deref() {
        Some([x, y, z]) => V3::new(*x, *y, *z),
        _ => V3::new(-1.0, 1.0, 1.0),
    };
    let v = if scene.light_world { View::from_scene(scene).apply(v) } else { v };
    v.unit().unwrap_or(V3::new(-1.0, 1.0, 1.0).unit().unwrap())
}

/// The tones of a lit surface with normal (nx, ny, nz) in view coordinates:
/// the palette of the orb (section 3.3 of docs/lighting.md), without the
/// reflected light.  Returns the weights to the base and lit tones and the
/// glint.
fn surface_terms(p: &mut Program, n: [Expr; 3], l: V3) -> (Expr, Expr, Expr) {
    let h = V3::new(l.x, l.y, l.z + 1.0).unit().unwrap();
    let [nx, ny, nz] = n;
    let dot = |v: V3| add(add(mul(nx.clone(), c(v.x)), mul(ny.clone(), c(v.y))), mul(nz.clone(), c(v.z)));
    let w = p.bind(clamp01(div(add(dot(l), c(ORB_WRAP)), c(1.0 + ORB_WRAP))));
    let to_base = p.bind(clamp01(mul(c(2.0), w.clone())));
    let to_lit = p.bind(clamp01(sub(mul(c(2.0), w), c(1.0))));
    let glint = p.bind(mul(c(ORB_GLINT_PEAK), pow(max(c(0.0), dot(h)), c(ORB_SHININESS))));
    (to_base, to_lit, glint)
}

/// The channels of a surface from its terms, and reflected light towards
/// the base colour.
fn surface_colour(
    slot: usize,
    (to_base, to_lit, glint): &(Expr, Expr, Expr),
    bounce: Option<&Expr>,
) -> [Expr; 3] {
    let channel = |ch: u8| {
        let core = mixed(slot, ch, ORB_CORE, false);
        let base = param(slot, ch);
        let lit = mixed(slot, ch, ORB_LIT, true);
        let body = add(
            add(core.clone(), mul(sub(base.clone(), core), to_base.clone())),
            mul(sub(lit, base.clone()), to_lit.clone()),
        );
        let body = match bounce {
            Some(b) => add(mul(body, sub(c(1.0), b.clone())), mul(base, b.clone())),
            None => body,
        };
        add(mul(body, sub(c(1.0), glint.clone())), mul(mixed(slot, ch, 0.06, true), glint.clone()))
    };
    [channel(0), channel(1), channel(2)]
}

fn shading_over(p: Program, region: &Path) -> Shading {
    let (center, extent) = domain(&region.sample(0.02));
    Shading { center, extent, program: p }
}

/// A sphere of `radius` about the page point `center`; `bounce` adds the
/// reflected light at its silhouette, for whole spheres.
fn sphere3(center: V3, radius: f64, slot: usize, l: V3, bounce: bool, region: &Path) -> Shading {
    let mut p = Program::new();
    let nx = p.bind(div(sub(Expr::X, c(center.x)), c(radius)));
    let ny = p.bind(div(sub(Expr::Y, c(center.y)), c(radius)));
    let r2 = p.bind(add(mul(nx.clone(), nx.clone()), mul(ny.clone(), ny.clone())));
    let nz = p.bind(sqrt(max(c(0.0), sub(c(1.0), r2))));
    let terms = surface_terms(&mut p, [nx.clone(), ny.clone(), nz.clone()], l);
    let reflected = bounce.then(|| {
        let flat = V3::xy(l.x, l.y).unit().unwrap_or(V3::xy(-1.0, 1.0).unit().unwrap());
        let away = p.bind(max(c(0.0), neg(add(mul(nx, c(flat.x)), mul(ny, c(flat.y))))));
        let tr = p.bind(clamp01(sub(c(1.0), div(nz, c(ORB_BOUNCE)))));
        p.bind(mul(c(ORB_BOUNCE_PEAK), mul(smooth(tr), away)))
    });
    p.output(surface_colour(slot, &terms, reflected.as_ref()));
    shading_over(p, region)
}

/// A flat fill of the base colour, under a solid's patches.
fn flat(slot: usize, region: &Path) -> Shading {
    let mut p = Program::new();
    p.output([param(slot, 0), param(slot, 1), param(slot, 2)]);
    shading_over(p, region)
}

/// The shading of one patch of a rounded solid.
fn patch_shading(patch: &Patch, slot: usize, l: V3) -> Shading {
    let region = patch.region();
    match patch {
        Patch::Sphere { center, radius } => sphere3(*center, *radius, slot, l, false, &region),
        Patch::Edge { a, axis, radius, b, .. } => {
            // Across the cylinder: e on the page, and w towards the viewer.
            let d = (*b - *a).unit().unwrap();
            let e = V3::xy(-d.y, d.x);
            let w = edge_toward(*axis, e);
            let mut p = Program::new();
            let q = p.bind(max(
                c(-1.0),
                min(
                    c(1.0),
                    div(
                        add(mul(sub(Expr::X, c(a.x)), c(e.x)), mul(sub(Expr::Y, c(a.y)), c(e.y))),
                        c(*radius),
                    ),
                ),
            ));
            let s = p.bind(sqrt(max(c(0.0), sub(c(1.0), mul(q.clone(), q.clone())))));
            let n = [
                add(mul(q.clone(), c(e.x)), mul(s.clone(), c(w.x))),
                add(mul(q.clone(), c(e.y)), mul(s.clone(), c(w.y))),
                mul(s, c(w.z)),
            ];
            let n = [p.bind(n[0].clone()), p.bind(n[1].clone()), p.bind(n[2].clone())];
            let terms = surface_terms(&mut p, n, l);
            p.output(surface_colour(slot, &terms, None));
            shading_over(p, &region)
        }
        Patch::Face { normal, .. } => {
            let mut p = Program::new();
            let terms = surface_terms(&mut p, [c(normal.x), c(normal.y), c(normal.z)], l);
            p.output(surface_colour(slot, &terms, None));
            shading_over(p, &region)
        }
    }
}

/// A tube in 3D: as `tube_shading`, but across each piece the normal turns
/// from the page towards the viewer about that piece's 3D axis.
fn tube_shading3(
    pieces: &[Piece],
    axes: &[V3],
    outline: Vec<V3>,
    radius: f64,
    slot: usize,
    l: V3,
) -> Shading {
    let (x, y) = (Expr::X, Expr::Y);
    let mut p = Program::new();
    let (mut bx, mut by, mut bd) = (c(0.0), c(0.0), c(FAR));
    let (mut wx, mut wy, mut wz) = (c(0.0), c(0.0), c(1.0));
    for (piece, axis) in pieces.iter().zip(axes) {
        let Piece::Line { a, b } = *piece else { continue };
        let len = (b - a).norm();
        let e = (b - a).unit().unwrap_or(V3::xy(1.0, 0.0));
        let across = V3::xy(-e.y, e.x);
        let mut w = V3::new(
            axis.y * across.z - axis.z * across.y,
            axis.z * across.x - axis.x * across.z,
            axis.x * across.y - axis.y * across.x,
        )
        .unit()
        .unwrap_or(V3::new(0.0, 0.0, 1.0));
        if w.z < 0.0 {
            w = -w;
        }
        let t = p.bind(min(
            max(add(mul(sub(x.clone(), c(a.x)), c(e.x)), mul(sub(y.clone(), c(a.y)), c(e.y))), c(0.0)),
            c(len),
        ));
        let dx = p.bind(sub(x.clone(), add(c(a.x), mul(t.clone(), c(e.x)))));
        let dy = p.bind(sub(y.clone(), add(c(a.y), mul(t, c(e.y)))));
        let d2 = p.bind(add(mul(dx.clone(), dx.clone()), mul(dy.clone(), dy.clone())));
        let closer = p.bind(lt(d2.clone(), bd.clone()));
        bx = p.bind(if_(closer.clone(), dx, bx));
        by = p.bind(if_(closer.clone(), dy, by));
        wx = p.bind(if_(closer.clone(), c(w.x), wx));
        wy = p.bind(if_(closer.clone(), c(w.y), wy));
        wz = p.bind(if_(closer, c(w.z), wz));
        bd = p.bind(min(d2, bd));
    }
    let qx = p.bind(div(bx, c(radius)));
    let qy = p.bind(div(by, c(radius)));
    let s =
        p.bind(sqrt(max(c(0.0), sub(c(1.0), add(mul(qx.clone(), qx.clone()), mul(qy.clone(), qy.clone()))))));
    let n = [p.bind(add(qx, mul(s.clone(), wx))), p.bind(add(qy, mul(s.clone(), wy))), p.bind(mul(s, wz))];
    let h = V3::new(l.x, l.y, l.z + 1.0).unit().unwrap();
    let dot =
        |v: V3| add(add(mul(n[0].clone(), c(v.x)), mul(n[1].clone(), c(v.y))), mul(n[2].clone(), c(v.z)));
    let diffuse = p.bind(max(c(0.0), dot(l)));
    let spec = p.bind(pow(max(c(0.0), dot(h)), c(TUBE_SHININESS)));
    let channel = |ch: u8| {
        min(
            c(1.0),
            add(
                mul(param(slot, ch), add(c(TUBE_AMBIENT), mul(c(TUBE_DIFFUSE), diffuse.clone()))),
                mul(c(TUBE_SPECULAR), spec.clone()),
            ),
        )
    };
    p.output([channel(0), channel(1), channel(2)]);
    let (center, extent) = domain(&outline);
    Shading { center, extent, program: p }
}

// ---- Tubes -----------------------------------------------------------------

/// A tube: every point takes the surface normal of a half-cylinder about the
/// nearest centreline point, lit by the world light raised above the page,
/// with ambient, diffuse, and Blinn-Phong terms in the tube's colour.
fn tube_shading(pieces: &[Piece], outline: Vec<V3>, radius: f64, slot: usize, light: f64) -> Shading {
    let (x, y) = (Expr::X, Expr::Y);
    let mut p = Program::new();
    let (mut bx, mut by, mut bd) = (c(0.0), c(0.0), c(FAR));
    for piece in pieces {
        let (dx, dy, d2) = match *piece {
            Piece::Line { a, b } => {
                let len = (b - a).norm();
                let e = (b - a).unit().unwrap_or(V3::xy(1.0, 0.0));
                let t = p.bind(min(
                    max(
                        add(mul(sub(x.clone(), c(a.x)), c(e.x)), mul(sub(y.clone(), c(a.y)), c(e.y))),
                        c(0.0),
                    ),
                    c(len),
                ));
                let dx = p.bind(sub(x.clone(), add(c(a.x), mul(t.clone(), c(e.x)))));
                let dy = p.bind(sub(y.clone(), add(c(a.y), mul(t, c(e.y)))));
                let d2 = add(mul(dx.clone(), dx.clone()), mul(dy.clone(), dy.clone()));
                (dx, dy, d2)
            }
            Piece::Arc { center, radius: rho, start, sweep } => {
                let vx = p.bind(sub(x.clone(), c(center.x)));
                let vy = p.bind(sub(y.clone(), c(center.y)));
                let (r0, r1) = (dir(start.to_degrees()), dir((start + sweep).to_degrees()));
                let sg = sweep.signum();
                let inside = p.bind(and(
                    lt(c(-1e-9), mul(c(sg), sub(mul(c(r0.x), vy.clone()), mul(c(r0.y), vx.clone())))),
                    lt(c(-1e-9), mul(c(sg), sub(mul(vx.clone(), c(r1.y)), mul(vy.clone(), c(r1.x))))),
                ));
                let len = add(sqrt(add(mul(vx.clone(), vx.clone()), mul(vy.clone(), vy.clone()))), c(1e-9));
                let f = p.bind(sub(c(1.0), div(c(rho), len)));
                let dx = p.bind(mul(vx, f.clone()));
                let dy = p.bind(mul(vy, f));
                let d2 = if_(inside, add(mul(dx.clone(), dx.clone()), mul(dy.clone(), dy.clone())), c(FAR));
                (dx, dy, d2)
            }
        };
        let d2 = p.bind(d2);
        let closer = p.bind(lt(d2.clone(), bd.clone()));
        bx = p.bind(if_(closer.clone(), dx, bx));
        by = p.bind(if_(closer.clone(), dy, by));
        bd = p.bind(min(d2, bd));
    }
    let nx = p.bind(div(bx, c(radius)));
    let ny = p.bind(div(by, c(radius)));
    let nz =
        p.bind(sqrt(max(c(0.0), sub(c(1.0), add(mul(nx.clone(), nx.clone()), mul(ny.clone(), ny.clone()))))));
    let el = TUBE_ELEVATION.to_radians();
    let l = V3::new(el.cos() * light.to_radians().cos(), el.cos() * light.to_radians().sin(), el.sin());
    let h = V3::new(l.x, l.y, l.z + 1.0).unit().unwrap();
    let lambert = |v: V3| add(add(mul(nx.clone(), c(v.x)), mul(ny.clone(), c(v.y))), mul(nz.clone(), c(v.z)));
    let diffuse = p.bind(max(c(0.0), lambert(l)));
    let spec = p.bind(pow(max(c(0.0), lambert(h)), c(TUBE_SHININESS)));
    let channel = |ch: u8| {
        min(
            c(1.0),
            add(
                mul(param(slot, ch), add(c(TUBE_AMBIENT), mul(c(TUBE_DIFFUSE), diffuse.clone()))),
                mul(c(TUBE_SPECULAR), spec.clone()),
            ),
        )
    };
    p.output([channel(0), channel(1), channel(2)]);
    let (center, extent) = domain(&outline);
    Shading { center, extent, program: p }
}

/// A cone ending a tube, lit like the tube: its normal leans towards the
/// tip by the cone's half-angle.
fn cone_shading(cone: &ArrowGeom, slot: usize, light: f64) -> Shading {
    let ArrowGeom::Cone { outline, base, axis, length, radius, .. } = cone else { unreachable!("a cone") };
    let n = V3::xy(-axis.y, axis.x);
    let half = radius.atan2(*length);
    let (ca, sa) = (half.cos(), half.sin());
    let mut p = Program::new();
    let dx = p.bind(sub(Expr::X, c(base.x)));
    let dy = p.bind(sub(Expr::Y, c(base.y)));
    let u = p.bind(add(mul(dx.clone(), c(axis.x)), mul(dy.clone(), c(axis.y))));
    let v = p.bind(add(mul(dx, c(n.x)), mul(dy, c(n.y))));
    let r = p.bind(add(mul(c(*radius), sub(c(1.0), clamp01(div(u, c(*length))))), c(1e-6)));
    let q = p.bind(max(c(-1.0), min(c(1.0), div(v, r))));
    let qz = p.bind(sqrt(max(c(0.0), sub(c(1.0), mul(q.clone(), q.clone())))));
    // The normal: the radial direction turned towards the axis by `half`.
    let nx = p.bind(add(mul(c(ca * n.x), q.clone()), c(sa * axis.x)));
    let ny = p.bind(add(mul(c(ca * n.y), q), c(sa * axis.y)));
    let nz = p.bind(mul(c(ca), qz));
    let el = TUBE_ELEVATION.to_radians();
    let l = V3::new(el.cos() * light.to_radians().cos(), el.cos() * light.to_radians().sin(), el.sin());
    let h = V3::new(l.x, l.y, l.z + 1.0).unit().unwrap();
    let dot = |v: V3| add(add(mul(nx.clone(), c(v.x)), mul(ny.clone(), c(v.y))), mul(nz.clone(), c(v.z)));
    let diffuse = p.bind(max(c(0.0), dot(l)));
    let spec = p.bind(pow(max(c(0.0), dot(h)), c(TUBE_SHININESS)));
    let channel = |ch: u8| {
        min(
            c(1.0),
            add(
                mul(param(slot, ch), add(c(TUBE_AMBIENT), mul(c(TUBE_DIFFUSE), diffuse.clone()))),
                mul(c(TUBE_SPECULAR), spec.clone()),
            ),
        )
    };
    p.output([channel(0), channel(1), channel(2)]);
    let (center, extent) = domain(&outline.sample(0.02));
    Shading { center, extent, program: p }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LabelSizes, geometry, layout, parse};

    fn lit(src: &str) -> (Geometry, Lighting) {
        let net = parse(src).unwrap();
        let lay = layout(&net).unwrap();
        let geom = geometry(&net, &lay, &GeometryOptions::default(), &LabelSizes::new()).unwrap();
        let light = lighting(&net, &geom, &GeometryOptions::default());
        (geom, light)
    }

    fn luminance(rgb: [f64; 3]) -> f64 {
        rgb[0] * LUMINANCE[0] + rgb[1] * LUMINANCE[1] + rgb[2] * LUMINANCE[2]
    }

    /// Evaluate a shading with every base colour a mid grey.
    fn at(s: &Shading, x: f64, y: f64) -> f64 {
        luminance(s.program.eval(x, y, &|_, _| 0.5))
    }

    #[test]
    fn glass_rims_face_the_light() {
        let (g, l) = lit("A at (0, 0)\nA [width=2, height=1.2, corner-radius=.2]\n");
        let face = &l.tensors[0].face;
        let t = &g.tensors[0];
        let (hw, hh) = (t.width / 2.0, t.height / 2.0);
        let inner = 0.01;
        let centre = at(face, 0.0, 0.0);
        // The upper edge faces the light (135°) and the lower one faces away.
        assert!(at(face, 0.0, hh - inner) > centre + 0.1);
        assert!(at(face, 0.0, -hh + inner) < centre - 0.05);
        assert!(at(face, -hw + inner, 0.0) > centre + 0.1);
        // The upper-left fillet, lit along its arc, is the brightest.
        let corner = V3::xy(-hw + 0.2, hh - 0.2) + V3::xy(-1.0, 1.0).unit().unwrap() * (0.2 - inner);
        assert!(at(face, corner.x, corner.y) >= at(face, 0.0, hh - inner));
        // The whole face stays in range.
        for i in -10..=10 {
            for j in -6..=6 {
                let rgb = face.program.eval(i as f64 * hw / 10.0, j as f64 * hh / 6.0, &|_, _| 0.5);
                assert!(rgb.iter().all(|v| (0.0..=1.0).contains(v)), "{rgb:?}");
            }
        }
    }

    #[test]
    fn corners_blend_without_a_seam() {
        // Along a line across the upper-right corner's bisector, brightness
        // changes smoothly: no step between neighbouring samples.
        let (g, l) = lit("A at (0, 0)\nA [width=2, height=2, corner-radius=0]\n");
        let face = &l.tensors[0].face;
        let _ = g;
        let samples: Vec<f64> =
            (0..=100).map(|k| at(face, 0.5 + 0.004 * k as f64, 0.9 - 0.004 * k as f64)).collect();
        let samples_across: Vec<f64> = (0..=100).map(|k| at(face, 0.6 + 0.002 * k as f64, 0.8)).collect();
        for s in [samples, samples_across] {
            for w in s.windows(2) {
                assert!((w[1] - w[0]).abs() < 0.02, "step {} to {}", w[0], w[1]);
            }
        }
    }

    #[test]
    fn tubes_are_lit_on_the_side_towards_the_light() {
        let (g, l) = lit("A at (0, 0)\nB at (4, 0)\nA - B [tube, width=.4]\n");
        let LineLook::Tube { shading, .. } = &l.lines[0] else { panic!("not a tube") };
        let _ = g;
        let (top, middle, bottom) = (at(shading, 2.0, 0.15), at(shading, 2.0, 0.0), at(shading, 2.0, -0.15));
        assert!(top > bottom && middle > bottom);
        // A vertical tube is lit on its left.
        let (_, l) = lit("A at (0, 0)\nB at (0, 4)\nA - B [tube, width=.4]\n");
        let LineLook::Tube { shading, .. } = &l.lines[0] else { panic!("not a tube") };
        assert!(at(shading, -0.15, 2.0) > at(shading, 0.15, 2.0));
    }

    #[test]
    fn colours_are_slots() {
        let (_, l) = lit("A [color=blue!64!black]\nB [color=blue!64!black]\nA - B [color=red]\n");
        assert_eq!(l.slots[..2], ["blue!64!black", "red"]);
        assert_eq!(l.tensors[0].outline.colour, Colour::mix(0, 0.76, Other::Black));
        let rgb = l.tensors[0].outline.colour.eval(&|_| [0.0, 0.0, 0.64]);
        assert!((rgb[2] - 0.64 * 0.76).abs() < 1e-12);
        let LabelColour::Auto { background, .. } = &l.labels[0] else { panic!("label colour") };
        assert_eq!(*background, 0);
    }

    #[test]
    fn compiled_shadings_match_evaluation() {
        let (g, l) = lit(
            "A at (0, 0)\nA [shape=triangle, corner-radius=.1]\nB at (3, 0)\nB [shape=orb]\nA - B [tube, via=(1.5, 1)]\n",
        );
        compiled_match(&g, &l);
    }

    /// The examples' shadings, with their long tubes and hops, run within
    /// PDF's stack limit.
    #[test]
    fn example_shadings_compile() {
        for src in [
            include_str!("../../../../examples/tnv/shapes.tnv"),
            include_str!("../../../../examples/tnv/routes.tnv"),
            include_str!("../../../../examples/tnv/sandwich.tnv"),
            include_str!("../../../../examples/tnv/ttn.tnv"),
            include_str!("../../../../examples/tnv/solids3d.tnv"),
            include_str!("../../../../examples/tnv/peps3d.tnv"),
            include_str!("../../../../examples/tnv/layers3d.tnv"),
        ] {
            let (g, l) = lit(src);
            compiled_match(&g, &l);
        }
    }

    /// Compiled code agrees with evaluation wherever a shading shows: inside
    /// its outline.  (Outside, a tube's nearest centreline point may jump.)
    fn compiled_match(g: &Geometry, l: &Lighting) {
        let mut shadings: Vec<(&Shading, &Path)> =
            l.tensors.iter().zip(&g.tensors).map(|(look, t)| (&look.face, &t.outline)).collect();
        for look in &l.tensors {
            shadings.extend(look.patches.iter().map(|(region, shading)| (shading, region)));
        }
        for (look, line) in l.lines.iter().zip(&g.lines) {
            if let (LineLook::Tube { shading, .. }, Some(outline)) = (look, &line.outline) {
                shadings.push((shading, outline));
            }
        }
        let param = |slot: usize, ch: u8| 0.3 + 0.1 * (slot as f64 + ch as f64);
        for (s, outline) in shadings {
            let code = s.program.to_postscript(&|slot, ch| expr::ps_number(param(slot, ch)));
            let polygon = outline.sample(0.01);
            let mut checked = 0;
            for i in 0..=40 {
                for j in 0..=40 {
                    let (x, y) = (
                        s.center.x + s.extent * (i as f64 / 20.0 - 1.0),
                        s.center.y + s.extent * (j as f64 / 20.0 - 1.0),
                    );
                    if !inside(&polygon, x, y) {
                        continue;
                    }
                    checked += 1;
                    let direct = s.program.eval(x, y, &param);
                    let stack = expr::ps::eval(&code, x, y);
                    for ch in 0..3 {
                        assert!(
                            // Constants have 6 decimals; high powers (glints, ^40) scale
                            // their rounding.  5e-4 is an eighth of a colour step.
                            (direct[ch] - stack[ch]).abs() < 5e-4,
                            "({x}, {y}): {} vs {}",
                            direct[ch],
                            stack[ch]
                        );
                    }
                }
            }
            assert!(checked > 0);
        }
    }

    fn inside(polygon: &[V3], x: f64, y: f64) -> bool {
        let mut inside = false;
        for k in 0..polygon.len() {
            let (a, b) = (polygon[k], polygon[(k + 1) % polygon.len()]);
            if (a.y > y) != (b.y > y) && x < a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
        inside
    }
}
