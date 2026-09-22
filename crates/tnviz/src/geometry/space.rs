//! The geometry of 3D scenes (docs/language.md, section 11).  Everything is
//! built in the world, then projected: the result is the same `Geometry` as
//! in 2D, in page coordinates, with depths, the visible patches of each
//! solid, the 3D direction of each piece of a line, and the spans in which
//! lines are drawn.

use super::solid::{Form, Solid, View};
use super::*;
use crate::layout::{PlacedBond, PlacedLeg};
use crate::model::PlanePlace;

fn dot(a: V3, b: V3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn cross(a: V3, b: V3) -> V3 {
    V3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}

/// A line in the world: a polyline, dense along its bends.
struct Line3 {
    points: Vec<V3>,
}

impl Line3 {
    fn length(&self) -> f64 {
        self.points.windows(2).map(|w| (w[1] - w[0]).norm()).sum()
    }

    fn at(&self, s: f64) -> V3 {
        let mut left = s.max(0.0);
        for w in self.points.windows(2) {
            let len = (w[1] - w[0]).norm();
            if left <= len {
                return if len > 0.0 { w[0] + (w[1] - w[0]) * (left / len) } else { w[0] };
            }
            left -= len;
        }
        *self.points.last().unwrap()
    }

    /// The part between arc lengths `s0` and `s1`.
    fn slice(&self, s0: f64, s1: f64) -> Line3 {
        let mut points = vec![self.at(s0)];
        let mut acc = 0.0;
        for w in self.points.windows(2) {
            acc += (w[1] - w[0]).norm();
            if acc > s0 + 1e-12 && acc < s1 - 1e-12 {
                points.push(w[1]);
            }
        }
        points.push(self.at(s1));
        Line3 { points }
    }

    /// The arc length at which the line, starting inside `solid`, leaves it.
    fn leaves(&self, solid: &Solid) -> f64 {
        let mut acc = 0.0;
        for w in self.points.windows(2) {
            let len = (w[1] - w[0]).norm();
            if solid.outside(w[1]) > 0.0 {
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..60 {
                    let mid = (lo + hi) / 2.0;
                    if solid.outside(w[0] + (w[1] - w[0]) * mid) > 0.0 {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return acc + len * (lo + hi) / 2.0;
            }
            acc += len;
        }
        acc
    }

    fn reversed(&self) -> Line3 {
        Line3 { points: self.points.iter().rev().copied().collect() }
    }
}

/// A polyline through `points` with its corners rounded by circular arcs in
/// the corners' planes (section 8.3, in 3D).
fn fillet3(points: &[V3], bend: f64) -> Line3 {
    let n = points.len();
    if n < 3 {
        return Line3 { points: points.to_vec() };
    }
    let mut out = vec![points[0]];
    for k in 1..n - 1 {
        let (p, prev, next) = (points[k], points[k - 1], points[k + 1]);
        let (lin, lout) = ((p - prev).norm(), (next - p).norm());
        let (Some(u), Some(v)) = ((p - prev).unit(), (next - p).unit()) else {
            out.push(p);
            continue;
        };
        let delta = dot(u, v).clamp(-1.0, 1.0).acos();
        if delta < 0.5f64.to_radians() || delta > std::f64::consts::PI - 1e-6 {
            out.push(p);
            continue;
        }
        let half = (delta / 2.0).tan();
        let rho = bend.min(0.5 * lin.min(lout) / half);
        let t = rho * half;
        let (t1, t2) = (p - u * t, p + v * t);
        let centre = p + (v - u).unit().unwrap() * (rho / (delta / 2.0).cos());
        let (a, b) = (t1 - centre, t2 - centre);
        let steps = ((delta / 10f64.to_radians()).ceil() as usize).max(3);
        // Slerp from a to b about the centre.
        for i in 0..=steps {
            let f = i as f64 / steps as f64;
            let w = ((1.0 - f) * delta).sin() / delta.sin();
            let z = (f * delta).sin() / delta.sin();
            out.push(centre + a * w + b * z);
        }
    }
    out.push(points[n - 1]);
    out.dedup_by(|a, b| (*a - *b).norm() < 1e-9);
    Line3 { points: out }
}

/// Where a tube of radius `r` leaving `solid` at its surface point `p`,
/// along `out`, meets the solid, on the page: for each line along the tube's
/// wall, the point where it leaves the solid, over the half of the wall that
/// faces the viewer.  At the start of a line (`start`) the curve runs from
/// the tube's right side to its left, at the end from left to right, as
/// `tube_region` wants.  None for a tube seen end on.
fn junction(solid: &Solid, p: V3, out: V3, r: f64, view: &View, start: bool) -> Option<Vec<V3>> {
    // Across the tube on the page (its left, along the line), and towards
    // the viewer, both perpendicular to the axis.
    let along = view.apply(if start { out } else { -out });
    let d = V3::xy(along.x, along.y).unit()?;
    let e = V3::xy(-d.y, d.x);
    let axis = view.apply(out).unit()?;
    let mut w = cross(axis, e).unit()?;
    if w.z < 0.0 {
        w = -w;
    }
    let (e, w) = (view.inverse(e), view.inverse(w));
    let steps = 24;
    let pts = (0..=steps)
        .map(|i| {
            let f = i as f64 / steps as f64;
            let theta = if start { std::f64::consts::PI * (1.0 - f) } else { std::f64::consts::PI * f };
            let base = p + (e * theta.cos() + w * theta.sin()) * r;
            // Along the wall line base + out t: inside at t_lo, outside at t_hi.
            let (mut lo, mut hi) = (-3.0 * r - 0.05, 3.0 * r + 0.05);
            let t = if solid.outside(base + out * lo) > 0.0 {
                0.0
            } else {
                while solid.outside(base + out * hi) <= 0.0 && hi < 1e3 {
                    hi *= 2.0;
                }
                for _ in 0..50 {
                    let mid = (lo + hi) / 2.0;
                    if solid.outside(base + out * mid) > 0.0 {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                (lo + hi) / 2.0
            };
            view.page(base + out * t)
        })
        .collect();
    Some(pts)
}

/// A projected line: its page centreline and the view direction of each
/// piece.
struct Projected {
    path: Path,
    axes: Vec<V3>,
    /// Where a tube meets its tensors (see `junction`).
    junctions: [Option<Vec<V3>>; 2],
}

fn project(line: &Line3, view: &View) -> Projected {
    let (mut pieces, mut axes) = (Vec::new(), Vec::new());
    for w in line.points.windows(2) {
        let (a, b) = (view.page(w[0]), view.page(w[1]));
        if (b - a).norm() < 1e-9 {
            continue;
        }
        pieces.push(Piece::Line { a, b });
        axes.push(view.apply(w[1] - w[0]).unit().unwrap());
    }
    if pieces.is_empty() {
        // A line pointing at the viewer: a short piece, so that it shows as
        // the disc of its cap.
        let a = view.page(line.points[0]);
        pieces.push(Piece::Line { a, b: a + V3::xy(1e-4, 0.0) });
        axes.push(V3::new(0.0, 0.0, 1.0));
    }
    Projected { path: Path { pieces, closed: false }, axes, junctions: [None, None] }
}

impl Builder<'_> {
    pub(super) fn space(&mut self) -> Result<Geometry> {
        let view = View::from_scene(self.net.scene());
        let lay = self.lay;
        let (solids, tensors, mut labels) = self.solids(&view)?;
        let mut lines = Vec::new();
        let mut worlds = Vec::new();
        for b in &lay.bonds {
            let (line, world) = self.bond3(b, &solids, &view);
            lines.push(line);
            worlds.push(world);
        }
        let offsets = self.leg_offsets3(&lay.legs, &solids);
        for (leg, &(offset, side)) in lay.legs.iter().zip(&offsets) {
            let (line, world) = self.leg3(leg, &solids, &view, solids[leg.tensor.0].center + side * offset);
            lines.push(line);
            worlds.push(world);
        }
        let (sheets, planes) = self.sheets(&view, &tensors, &solids)?;
        let mut tensors = tensors;
        for (k, t) in tensors.iter_mut().enumerate() {
            t.layer = layer(&sheets, solids[k].center, Some(TensorId(k)), None);
        }
        for (k, line) in lines.iter().enumerate() {
            if let Some(label) = self.line_label(k, line) {
                labels.push(label);
            }
            labels.extend(self.end_labels(k, line));
        }
        for (k, line) in lines.iter_mut().enumerate() {
            let label = labels.iter().find(|l| l.owner == LabelOwner::Line(k));
            line.arrow = self.arrow(line, label);
            if let (Some(ArrowGeom::Cone { base_at, forward, .. }), Some(_)) = (&line.arrow, &line.outline) {
                let total = line.centerline.length();
                let (s0, s1, caps) = if *forward {
                    (0.0, *base_at, (line.caps.0, Cap::Flat))
                } else {
                    (*base_at, total, (Cap::Flat, line.caps.1))
                };
                // The other end keeps its junction with its tensor.
                let j = if *forward {
                    [line.junctions[0].as_deref(), None]
                } else {
                    [None, line.junctions[1].as_deref()]
                };
                line.outline = Some(tube_region(&line.centerline.slice(s0, s1), line.width / 2.0, caps, j));
            }
        }
        let spans: Vec<Vec<Span>> =
            (0..lines.len()).map(|k| spans(k, &lines, &worlds[k], &tensors, &sheets, &view)).collect();
        for (line, s) in lines.iter_mut().zip(spans) {
            line.spans = s;
        }
        Ok(Geometry {
            tensors,
            lines,
            labels,
            crossings: Vec::new(),
            planes,
            warnings: std::mem::take(&mut self.warnings),
        })
    }

    /// The solids of section 11.3, their outlines and patches, and the
    /// tensors' labels.
    fn solids(&mut self, view: &View) -> Result<(Vec<Solid>, Vec<TensorGeom>, Vec<LabelGeom>)> {
        let (mut solids, mut out, mut labels) = (Vec::new(), Vec::new(), Vec::new());
        for (id, t) in self.net.tensors() {
            let style = self.net.tensor_style(id);
            let shape_word = word(&style, "shape").unwrap_or("box");
            let shape = Shape::from_word(shape_word).filter(|s| s.is_3d()).ok_or_else(|| {
                Error::new(format!("{}: `{shape_word}` is a 2D shape, in a 3D scene", t.name))
            })?;
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
            // Sized by its 2D cross-section (section 11.3).
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
            let thick = self.length(&style, "thickness", false).unwrap_or(shape.default_thickness());
            let corner = self.length(&style, "corner-radius", false).unwrap_or(0.12);
            let form = match shape {
                Shape::Box => Form::rounded_box(w, h, thick, corner),
                Shape::Prism => Form::prism(w, h, thick, corner),
                Shape::Octahedron => Form::octahedron(w, h, thick, corner),
                _ => Form::ball(w),
            };
            let used = match &form {
                Form::Rounded { radius, .. } => *radius,
                Form::Ball { .. } => 0.0,
            };
            let placed = &self.lay.tensors[id.0];
            let solid = Solid { center: placed.pos, orient: placed.orient, form };
            let page = view.page(placed.pos);
            out.push(TensorGeom {
                tensor: id,
                shape,
                center: page,
                rotation: 0.0,
                width: w,
                height: h,
                corner_radius: used,
                outline: solid.silhouette(view),
                depth: view.depth(placed.pos),
                patches: solid.patches(view),
                layer: 0,
            });
            solids.push(solid);
            if let Some(((size, measured), text)) = label {
                labels.push(LabelGeom {
                    id: label_id,
                    owner: LabelOwner::Tensor(id),
                    on_line: false,
                    text,
                    pos: page,
                    angle: 0.0,
                    anchor: Anchor::Center,
                    size,
                    measured,
                });
            }
        }
        Ok((solids, out, labels))
    }

    fn bond3(&mut self, b: &PlacedBond, solids: &[Solid], view: &View) -> (LineGeom, Line3) {
        let attrs = self.net.bond_style(b.index);
        let (style, width, wref) = self.line_style(&attrs);
        let bend = self.length(&attrs, "bend-radius", false).unwrap_or(0.2f64.max(0.8 * wref));
        let stub = self.length(&attrs, "stub", false).unwrap_or(bend + wref / 2.0);
        let gap = self.length(&attrs, "parallel-gap", false).unwrap_or(0.3f64.max(1.5 * wref));
        let loop_size = self.length(&attrs, "loop-size", false).unwrap_or(0.45);
        if word(&attrs, "crossing") == Some("hop") {
            let i = self.net.index(b.index);
            self.warnings.push(format!(
                "bond {}: hops are 2D only; in 3D depth shows which line is in front",
                index_display(&i.name, i.prime)
            ));
        }
        let (sa, sb) = (&solids[b.a.tensor.0], &solids[b.b.tensor.0]);
        let (ca, cb) = (sa.center, sb.center);
        let mut points = vec![ca];
        if let Some(d) = b.a.dir {
            points.push(ca + d * (sa.exit(ca, d) + stub));
        }
        match &b.route {
            Route::Direct => {}
            Route::Via(v) => points.extend(v),
            Route::Parallel { k, n } => {
                // Apart on the page: across the bond and the view direction.
                let side = *k as f64 - (*n - 1) as f64 / 2.0;
                let toward = view.inverse(V3::new(0.0, 0.0, 1.0));
                let normal = cross(toward, cb - ca).unit().unwrap_or(V3::new(0.0, 0.0, 1.0));
                points.push((ca + cb) * 0.5 + normal * (side * gap));
            }
            Route::Loop { k } => {
                let t = &self.lay.tensors[b.a.tensor.0];
                let geom_h = match &sa.form {
                    Form::Ball { radius } => 2.0 * radius,
                    Form::Rounded { .. } => sa.exit(ca, t.orient.apply(V3::xy(0.0, 1.0))) * 2.0,
                };
                let geom_w = match &sa.form {
                    Form::Ball { radius } => 2.0 * radius,
                    Form::Rounded { .. } => sa.exit(ca, t.orient.apply(V3::xy(1.0, 0.0))) * 2.0,
                };
                let y = geom_h / 2.0 + (*k + 1) as f64 * loop_size;
                for x in [-0.35 * geom_w, 0.35 * geom_w] {
                    points.push(ca + t.orient.apply(V3::xy(x, y)));
                }
            }
        }
        if let Some(d) = b.b.dir {
            points.push(cb + d * (sb.exit(cb, d) + stub));
        }
        points.push(cb);
        points.dedup_by(|p, q| (*p - *q).norm() < 1e-4);
        let full = fillet3(&points, bend);
        // Cut where it enters its end tensors (section 11.4).
        let from = full.leaves(sa);
        let to = full.length() - full.reversed().leaves(sb);
        let visible = if to > from + 1e-6 { full.slice(from, to) } else { full.slice(from, from + 1e-4) };
        let mut proj = project(&visible, view);
        let caps = (Cap::Round, Cap::Round);
        proj.junctions = if style == LineStyle::Tube {
            let n = visible.points.len();
            let (p0, p1) = (visible.points[0], visible.points[n - 1]);
            [
                (visible.points[1] - p0)
                    .unit()
                    .and_then(|out| junction(sa, p0, out, width / 2.0, view, true)),
                (visible.points[n - 2] - p1)
                    .unit()
                    .and_then(|out| junction(sb, p1, out, width / 2.0, view, false)),
            ]
        } else {
            [None, None]
        };
        (self.line3(b.index, LineKind::Bond, style, width, caps, proj), visible)
    }

    /// Where open legs leave their solids: as `leg_offsets`, across the
    /// solid, perpendicular to the leg in reading order in the local frame
    /// (to the local x, or down when the leg runs along x).
    fn leg_offsets3(&self, legs: &[PlacedLeg], solids: &[Solid]) -> Vec<(f64, V3)> {
        let mut out = Vec::with_capacity(legs.len());
        let mut groups: BTreeMap<(usize, i64, i64, i64), Vec<usize>> = Default::default();
        for (k, leg) in legs.iter().enumerate() {
            let placed = &self.lay.tensors[leg.tensor.0];
            let d = placed.orient.inverse().apply(leg.dir);
            let along_x = V3::new(1.0, 0.0, 0.0) - d * d.x;
            let p = along_x.unit().unwrap_or_else(|| (V3::new(0.0, -1.0, 0.0) - d * -d.y).unit().unwrap());
            let side = placed.orient.apply(p);
            let attrs = self.net.leg_style(leg.tensor, leg.slot);
            match self.length(&attrs, "leg-offset", false) {
                Some(offset) => out.push((offset, side)),
                None => {
                    out.push((0.0, side));
                    let r = |x: f64| (x * 1e6).round() as i64;
                    groups.entry((leg.tensor.0, r(d.x), r(d.y), r(d.z))).or_default().push(k);
                }
            }
        }
        for members in groups.values_mut() {
            if members.len() < 2 {
                continue;
            }
            members.sort_by_key(|&k| legs[k].slot);
            let (t, side) = (legs[members[0]].tensor, out[members[0]].1);
            let s = &solids[t.0];
            let r = match &s.form {
                Form::Rounded { radius, .. } => *radius,
                Form::Ball { .. } => 0.0,
            };
            let lo = -s.exit(s.center, -side) + r;
            let hi = s.exit(s.center, side) - r;
            let (lo, hi) = if hi > lo { (lo, hi) } else { (0.0, 0.0) };
            let n = members.len() as f64;
            for (i, &k) in members.iter().enumerate() {
                out[k].0 = lo + (i as f64 + 0.5) * (hi - lo) / n;
            }
        }
        out
    }

    fn leg3(&mut self, leg: &PlacedLeg, solids: &[Solid], view: &View, start: V3) -> (LineGeom, Line3) {
        let attrs = self.net.leg_style(leg.tensor, leg.slot);
        let (style, width, _) = self.line_style(&attrs);
        let length = self.length(&attrs, "leg-length", false).unwrap_or(0.6);
        let first = start + leg.dir * solids[leg.tensor.0].exit(start, leg.dir);
        let line = Line3 { points: vec![first, first + leg.dir * length] };
        let proj = project(&line, view);
        let cap = if word(&attrs, "cap") == Some("flat") { Cap::Flat } else { Cap::Round };
        let mut proj = proj;
        proj.junctions = if style == LineStyle::Tube {
            [junction(&solids[leg.tensor.0], first, leg.dir, width / 2.0, view, true), None]
        } else {
            [None, None]
        };
        (self.line3(leg.index, LineKind::Leg, style, width, (Cap::Round, cap), proj), line)
    }

    fn line3(
        &self,
        index: IndexId,
        kind: LineKind,
        style: LineStyle,
        width: f64,
        caps: (Cap, Cap),
        proj: Projected,
    ) -> LineGeom {
        let junctions = proj.junctions;
        let length = proj.path.length();
        let outline = (style == LineStyle::Tube).then(|| {
            tube_region(&proj.path, width / 2.0, caps, [junctions[0].as_deref(), junctions[1].as_deref()])
        });
        LineGeom {
            index,
            kind,
            style,
            width,
            centerline: proj.path,
            outline,
            caps,
            visible: (0.0, length),
            arrow: None,
            axes: proj.axes,
            spans: Vec::new(),
            junctions,
        }
    }
}

/// A plane in the world: its centre, its normal towards the viewer, and
/// the tensors resting on it.
struct Sheet {
    center: V3,
    normal: V3,
    members: Vec<TensorId>,
}

/// The layer of a point among the planes (section 11.7): twice the number
/// of planes it is in front of, counting those it lies in or rests on.
/// `own` leaves out a plane itself, for its own centre.
fn layer(sheets: &[Sheet], p: V3, tensor: Option<TensorId>, own: Option<usize>) -> u32 {
    let front = sheets
        .iter()
        .enumerate()
        .filter(|(k, _)| Some(*k) != own)
        .filter(|(_, sh)| {
            tensor.is_some_and(|t| sh.members.contains(&t)) || dot(p - sh.center, sh.normal) > -1e-6
        })
        .count();
    2 * front as u32
}

impl Builder<'_> {
    /// The planes of section 11.6, in the world and on the page.
    fn sheets(
        &mut self,
        view: &View,
        tensors: &[TensorGeom],
        solids: &[Solid],
    ) -> Result<(Vec<Sheet>, Vec<PlaneGeom>)> {
        let mut sheets = Vec::new();
        let mut outlines = Vec::new();
        for (k, plane) in self.net.planes().iter().enumerate() {
            let attrs = self.net.plane_style(k);
            let corner = self.length(&attrs, "corner-radius", false).unwrap_or(0.12);
            let (center, u, v, n, w, h, members) = match &plane.place {
                PlanePlace::Under(g) => {
                    let group = self
                        .net
                        .group(g)
                        .ok_or_else(|| Error::new(format!("plane {}: no group `{g}`", plane.name)))?;
                    let members = group.members.clone();
                    let pts: Vec<V3> = members.iter().map(|t| solids[t.0].center).collect();
                    let n = fit_normal(&pts).ok_or_else(|| {
                        Error::new(format!("plane {}: the tensors of `{g}` are not in one plane", plane.name))
                    })?;
                    let u = (V3::new(1.0, 0.0, 0.0) - n * n.x)
                        .unit()
                        .or_else(|| (V3::new(0.0, 1.0, 0.0) - n * n.y).unit())
                        .unwrap();
                    let v = cross(n, u);
                    let pad = self.length(&attrs, "padding", false).unwrap_or(0.4);
                    let (mut lo, mut hi) =
                        ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
                    for t in &members {
                        let c = solids[t.0].center - pts[0];
                        let reach = tensors[t.0].width.max(tensors[t.0].height) / 2.0 + pad;
                        let (a, b) = (dot(c, u), dot(c, v));
                        lo = (lo.0.min(a - reach), lo.1.min(b - reach));
                        hi = (hi.0.max(a + reach), hi.1.max(b + reach));
                    }
                    let mid = pts[0] + u * ((lo.0 + hi.0) / 2.0) + v * ((lo.1 + hi.1) / 2.0);
                    (mid, u, v, n, hi.0 - lo.0, hi.1 - lo.1, members)
                }
                PlanePlace::At(at) => {
                    let c = V3::new(at[0], at[1], at.get(2).copied().unwrap_or(0.0));
                    let rot = crate::layout::Rot::from_angles(
                        registry::rotation(&attrs, "rotate").unwrap_or([0.0; 3]),
                    );
                    let w = self.length(&attrs, "width", false).unwrap_or(4.0);
                    let h = self.length(&attrs, "height", false).unwrap_or(3.0);
                    let axis = |x, y, z| rot.apply(V3::new(x, y, z));
                    (c, axis(1.0, 0.0, 0.0), axis(0.0, 1.0, 0.0), axis(0.0, 0.0, 1.0), w, h, Vec::new())
                }
            };
            // Towards the viewer, to tell its sides apart.
            let n = if view.apply(n).z < 0.0 { -n } else { n };
            let corners = [
                V3::xy(-w / 2.0, -h / 2.0),
                V3::xy(w / 2.0, -h / 2.0),
                V3::xy(w / 2.0, h / 2.0),
                V3::xy(-w / 2.0, h / 2.0),
            ];
            let (flat, _) = path::fillet_closed(&corners, corner.max(0.0));
            let pts: Vec<V3> =
                flat.sample(0.05).iter().map(|q| view.page(center + u * q.x + v * q.y)).collect();
            let m = pts.len();
            let outline = Path {
                pieces: (0..m).map(|i| Piece::Line { a: pts[i], b: pts[(i + 1) % m] }).collect(),
                closed: true,
            };
            outlines.push((outline, view.depth(center)));
            sheets.push(Sheet { center, normal: n, members });
        }
        let planes = outlines
            .into_iter()
            .enumerate()
            .map(|(k, (outline, depth))| PlaneGeom {
                outline,
                depth,
                layer: layer(&sheets, sheets[k].center, None, Some(k)) + 1,
            })
            .collect();
        Ok((sheets, planes))
    }
}

/// The normal of the plane through points, if they are in one; for points
/// on a line, of the plane containing it with the normal nearest +z; for one
/// point, +z.
fn fit_normal(pts: &[V3]) -> Option<V3> {
    let z = V3::new(0.0, 0.0, 1.0);
    let a = pts[0];
    let far = pts.iter().copied().max_by(|p, q| (*p - a).norm().total_cmp(&(*q - a).norm()));
    let Some(b) = far.filter(|b| (*b - a).norm() > 1e-9) else {
        return Some(z);
    };
    let d = (b - a).unit().unwrap();
    let off = |p: V3| {
        let r = p - a;
        (r - d * dot(r, d)).norm()
    };
    let c = pts.iter().copied().max_by(|p, q| off(*p).total_cmp(&off(*q))).unwrap();
    if off(c) < 1e-6 {
        return (z - d * dot(z, d)).unit().or_else(|| (V3::new(0.0, 1.0, 0.0) - d * d.y).unit());
    }
    let n = cross(b - a, c - a).unit().unwrap();
    let n = if n.z < 0.0 { -n } else { n };
    pts.iter().all(|p| dot(*p - a, n).abs() < 1e-6).then_some(n)
}

/// The spans of line `k` (section 11.7): cut where its page centreline
/// crosses another line or a silhouette, and where it crosses a plane; each
/// at the depth, and in the layer, of its middle.
fn spans(
    k: usize,
    lines: &[LineGeom],
    world: &Line3,
    tensors: &[TensorGeom],
    sheets: &[Sheet],
    view: &View,
) -> Vec<Span> {
    let line = &lines[k];
    let mut cuts = vec![0.0, line.centerline.length()];
    let mut offset = 0.0;
    for piece in &line.centerline.pieces {
        let others = lines
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != k)
            .flat_map(|(_, l)| l.centerline.pieces.iter())
            .chain(tensors.iter().flat_map(|t| t.outline.pieces.iter()));
        for other in others {
            for (_, s, _) in piece.intersect(other) {
                cuts.push(offset + s);
            }
        }
        offset += piece.length();
    }
    // The world segments with their page lengths, for going between arc
    // length on the page and points in the world.
    let segs: Vec<(V3, V3, f64)> =
        world.points.windows(2).map(|w| (w[0], w[1], (view.page(w[1]) - view.page(w[0])).norm())).collect();
    let mut acc = 0.0;
    for &(a, b, len) in &segs {
        for sh in sheets {
            let (da, db) = (dot(a - sh.center, sh.normal), dot(b - sh.center, sh.normal));
            if da * db < 0.0 {
                cuts.push(acc + len * da / (da - db));
            }
        }
        acc += len;
    }
    let world_at = |s: f64| {
        let mut left = s;
        for &(a, b, len) in &segs {
            if len > 1e-12 && left <= len {
                return a + (b - a) * (left / len);
            }
            left -= len;
        }
        *world.points.last().unwrap()
    };
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    cuts.windows(2)
        .map(|w| {
            let mid = world_at((w[0] + w[1]) / 2.0);
            Span { from: w[0], to: w[1], depth: view.depth(mid), layer: layer(sheets, mid, None, None) }
        })
        .collect()
}
