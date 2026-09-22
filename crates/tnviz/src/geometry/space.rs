//! The geometry of 3D scenes (docs/language.md, section 11).  Everything is
//! built in the world, then projected: the result is the same `Geometry` as
//! in 2D, in page coordinates, with the visible patches of each solid, the 3D
//! direction of each piece of a line, and where each object is drawn, from
//! the compositor.

use super::compose::{Depth, Surface, compose, cover};
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

/// A projected line: its page centreline and the view direction of each
/// piece.
struct Projected {
    path: Path,
    axes: Vec<V3>,
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
    Projected { path: Path { pieces, closed: false }, axes }
}

/// The page arc length of the point at world arc length `s` of a line.
fn page_arc(line: &Line3, view: &View, s: f64) -> f64 {
    let (mut left, mut page) = (s, 0.0);
    for w in line.points.windows(2) {
        let len = (w[1] - w[0]).norm();
        let plen = (view.page(w[1]) - view.page(w[0])).norm();
        if plen < 1e-9 {
            left -= len;
            continue;
        }
        if left <= len {
            return page + plen * (left / len.max(1e-12));
        }
        left -= len;
        page += plen;
    }
    page
}

/// A line in the world with where its visible part is on the page.
struct Built {
    line: LineGeom,
    world: Line3,
}

impl Builder<'_> {
    pub(super) fn space(&mut self) -> Result<Geometry> {
        let view = View::from_scene(self.net.scene());
        let lay = self.lay;
        let (solids, mut tensors, mut labels) = self.solids(&view)?;
        let mut built = Vec::new();
        for b in &lay.bonds {
            built.push(self.bond3(b, &solids, &view));
        }
        let offsets = self.leg_offsets3(&lay.legs, &solids);
        for (leg, &(offset, side)) in lay.legs.iter().zip(&offsets) {
            built.push(self.leg3(leg, &solids, &view, solids[leg.tensor.0].center + side * offset));
        }
        let (mut lines, worlds): (Vec<LineGeom>, Vec<Line3>) =
            built.into_iter().map(|b| (b.line, b.world)).unzip();
        for (k, line) in lines.iter().enumerate() {
            if let Some(label) = self.line_label(k, line) {
                labels.push(label);
            }
            labels.extend(self.end_labels(k, line));
        }
        for (k, line) in lines.iter_mut().enumerate() {
            let label = labels.iter().find(|l| l.owner == LabelOwner::Line(k));
            line.arrow = self.arrow(line, label);
            // A cone ends its tube.
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
        let (mut planes, sheets) = self.planes3(&view, &tensors, &solids)?;

        // Every object and plane as a surface, for the compositor.
        let mut objects: Vec<Surface> = solids
            .iter()
            .zip(&tensors)
            .map(|(s, t)| Surface {
                region: t.outline.sample(0.01),
                depth: Depth::Solid(s.clone()),
                key: view.depth(s.center),
            })
            .collect();
        for (line, world) in lines.iter().zip(&worlds) {
            let r = line.width / 2.0;
            let region = match &line.outline {
                Some(o) => o.sample(0.01),
                None => tube_outline(&line.centerline, r, line.caps).sample(0.01),
            };
            let pts: Vec<V3> = world.points.iter().map(|p| view.apply(*p)).collect();
            let key = pts.iter().map(|p| p.z).sum::<f64>() / pts.len().max(1) as f64;
            objects.push(Surface { region, depth: Depth::Tube { pts, r }, key });
        }
        let sheet_surfaces: Vec<Surface> = sheets
            .iter()
            .zip(&planes)
            .map(|((c, n), p)| Surface {
                region: p.outline.sample(0.01),
                depth: Depth::Plane { c: view.apply(*c), n: view.apply(*n) },
                key: view.depth(*c),
            })
            .collect();
        let composed = compose(&view, &objects, &sheet_surfaces);
        let mut drawn =
            composed.rank.into_iter().zip(composed.holes).map(|(rank, holes)| Drawn { rank, holes });
        for t in &mut tensors {
            t.drawn = drawn.next().unwrap();
        }
        for l in &mut lines {
            l.drawn = drawn.next().unwrap();
        }
        for p in &mut planes {
            p.drawn = drawn.next().unwrap();
        }
        self.cards(&view, &solids, &tensors, &lines, &worlds, &objects, &mut labels);
        // The frame round everything, shadows included.
        let pts: Vec<V3> =
            objects.iter().chain(&sheet_surfaces).flat_map(|s| s.region.iter().copied()).collect();
        let frame = (!pts.is_empty()).then(|| {
            let (lo, hi) =
                pts.iter().fold((V3::xy(f64::MAX, f64::MAX), V3::xy(f64::MIN, f64::MIN)), |(lo, hi), p| {
                    (V3::xy(lo.x.min(p.x), lo.y.min(p.y)), V3::xy(hi.x.max(p.x), hi.y.max(p.y)))
                });
            (lo - V3::xy(0.2, 0.2), hi + V3::xy(0.2, 0.2))
        });
        Ok(Geometry {
            tensors,
            lines,
            labels,
            crossings: Vec::new(),
            planes,
            frame,
            warnings: std::mem::take(&mut self.warnings),
        })
    }

    /// Labels are cards facing the viewer, drawn above everything, far
    /// first: each at the depth of what it is printed on, and covered only
    /// by other tensors nearer than it (language section 11.7).
    #[allow(clippy::too_many_arguments)]
    fn cards(
        &self,
        view: &View,
        solids: &[Solid],
        tensors: &[TensorGeom],
        lines: &[LineGeom],
        worlds: &[Line3],
        objects: &[Surface],
        labels: &mut [LabelGeom],
    ) {
        let mut cards = Vec::new();
        for label in labels.iter() {
            let (w, h, d) = label.size;
            let (hw, hh) = (w / 2.0, (h + d) / 2.0);
            let centre = match &label.anchor {
                Anchor::Center => label.pos,
                Anchor::Toward(dir) => {
                    let u = dir.unit().unwrap_or(V3::xy(1.0, 0.0));
                    let t = (hw / u.x.abs().max(1e-9)).min(hh / u.y.abs().max(1e-9));
                    label.pos - u * t
                }
            };
            // Generous, for any rotation: only the holes clip the label.
            let reach = (hw * hw + hh * hh).sqrt() + 0.1;
            let region = vec![
                centre + V3::xy(-reach, -reach),
                centre + V3::xy(reach, -reach),
                centre + V3::xy(reach, reach),
                centre + V3::xy(-reach, reach),
            ];
            let (z, owner) = match label.owner {
                LabelOwner::Tensor(t) => {
                    let k = tensors.iter().position(|g| g.tensor == t).unwrap();
                    let s = &solids[k];
                    let z = s.surface_depth(view, label.pos).unwrap_or(view.depth(s.center) + s.reach());
                    (z, Some(k))
                }
                LabelOwner::Line(l) => {
                    let pts: Vec<V3> = worlds[l].points.iter().map(|p| view.apply(*p)).collect();
                    let r = lines[l].width / 2.0;
                    let tube = Depth::Tube { pts: pts.clone(), r };
                    let z = if label.on_line { tube.at_point(view, label.pos) } else { None };
                    (z.unwrap_or_else(|| nearest_depth(&pts, label.pos) + r), None)
                }
            };
            let card = Surface {
                region,
                depth: Depth::Plane { c: V3::new(label.pos.x, label.pos.y, z), n: V3::new(0.0, 0.0, 1.0) },
                key: z,
            };
            let over: Vec<&Surface> = objects[..tensors.len()]
                .iter()
                .enumerate()
                .filter(|(k, _)| Some(*k) != owner)
                .map(|(_, s)| s)
                .collect();
            let holes = cover(view, &card, &over);
            cards.push((z, holes));
        }
        let mut order: Vec<usize> = (0..cards.len()).collect();
        order.sort_by(|&a, &b| cards[a].0.total_cmp(&cards[b].0));
        for (rank, k) in order.into_iter().enumerate() {
            labels[k].drawn = Drawn { rank: rank as u32, holes: std::mem::take(&mut cards[k].1) };
        }
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
                drawn: Drawn::default(),
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
                    drawn: Drawn::default(),
                });
            }
        }
        Ok((solids, out, labels))
    }

    fn bond3(&mut self, b: &PlacedBond, solids: &[Solid], view: &View) -> Built {
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
                let size = |d: V3| match &sa.form {
                    Form::Ball { radius } => 2.0 * radius,
                    Form::Rounded { .. } => sa.exit(ca, t.orient.apply(d)) * 2.0,
                };
                let (w, h) = (size(V3::xy(1.0, 0.0)), size(V3::xy(0.0, 1.0)));
                let y = h / 2.0 + (*k + 1) as f64 * loop_size;
                for x in [-0.35 * w, 0.35 * w] {
                    points.push(ca + t.orient.apply(V3::xy(x, y)));
                }
            }
        }
        if let Some(d) = b.b.dir {
            points.push(cb + d * (sb.exit(cb, d) + stub));
        }
        points.push(cb);
        points.dedup_by(|p, q| (*p - *q).norm() < 1e-4);
        // From centre to centre: the tensors hide what is inside them.
        let world = fillet3(&points, bend);
        let from = world.leaves(sa);
        let to = (world.length() - world.reversed().leaves(sb)).max(from);
        let visible = (page_arc(&world, view, from), page_arc(&world, view, to));
        let line = self.line3(
            b.index,
            LineKind::Bond,
            style,
            width,
            (Cap::Round, Cap::Round),
            project(&world, view),
            visible,
        );
        Built { line, world }
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

    fn leg3(&mut self, leg: &PlacedLeg, solids: &[Solid], view: &View, start: V3) -> Built {
        let attrs = self.net.leg_style(leg.tensor, leg.slot);
        let (style, width, _) = self.line_style(&attrs);
        let length = self.length(&attrs, "leg-length", false).unwrap_or(0.6);
        let inside = solids[leg.tensor.0].exit(start, leg.dir);
        let world = Line3 { points: vec![start, start + leg.dir * (inside + length)] };
        let visible = (page_arc(&world, view, inside), page_arc(&world, view, inside + length));
        let cap = if word(&attrs, "cap") == Some("flat") { Cap::Flat } else { Cap::Round };
        let line = self.line3(
            leg.index,
            LineKind::Leg,
            style,
            width,
            (Cap::Round, cap),
            project(&world, view),
            visible,
        );
        Built { line, world }
    }

    #[allow(clippy::too_many_arguments)]
    fn line3(
        &self,
        index: IndexId,
        kind: LineKind,
        style: LineStyle,
        width: f64,
        caps: (Cap, Cap),
        proj: Projected,
        visible: (f64, f64),
    ) -> LineGeom {
        let outline = (style == LineStyle::Tube).then(|| tube_outline(&proj.path, width / 2.0, caps));
        LineGeom {
            index,
            kind,
            style,
            width,
            centerline: proj.path,
            outline,
            caps,
            visible,
            arrow: None,
            axes: proj.axes,
            drawn: Drawn::default(),
        }
    }

    /// The planes of section 11.6 on the page, and each one's centre and
    /// normal in the world, towards the viewer.
    #[allow(clippy::type_complexity)]
    fn planes3(
        &mut self,
        view: &View,
        tensors: &[TensorGeom],
        solids: &[Solid],
    ) -> Result<(Vec<PlaneGeom>, Vec<(V3, V3)>)> {
        let mut planes = Vec::new();
        let mut sheets = Vec::new();
        for (k, plane) in self.net.planes().iter().enumerate() {
            let attrs = self.net.plane_style(k);
            let corner = self.length(&attrs, "corner-radius", false).unwrap_or(0.12);
            let (center, u, v, n, w, h) = match &plane.place {
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
                    (mid, u, v, n, hi.0 - lo.0, hi.1 - lo.1)
                }
                PlanePlace::At(at) => {
                    let c = V3::new(at[0], at[1], at.get(2).copied().unwrap_or(0.0));
                    let rot = crate::layout::Rot::from_angles(
                        registry::rotation(&attrs, "rotate").unwrap_or([0.0; 3]),
                    );
                    let w = self.length(&attrs, "width", false).unwrap_or(4.0);
                    let h = self.length(&attrs, "height", false).unwrap_or(3.0);
                    let axis = |x, y, z| rot.apply(V3::new(x, y, z));
                    (c, axis(1.0, 0.0, 0.0), axis(0.0, 1.0, 0.0), axis(0.0, 0.0, 1.0), w, h)
                }
            };
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
            planes.push(PlaneGeom { outline, depth: view.depth(center), drawn: Drawn::default() });
            sheets.push((center, n));
        }
        Ok((planes, sheets))
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

/// The depth, in view coordinates, of the point of the polyline `pts`
/// nearest `p` on the page.
fn nearest_depth(pts: &[V3], p: V3) -> f64 {
    let mut best = (f64::MAX, pts.first().map_or(0.0, |q| q.z));
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = V3::xy(b.x - a.x, b.y - a.y);
        let len2 = d.x * d.x + d.y * d.y;
        let t =
            if len2 > 1e-18 { (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let q = a + (b - a) * t;
        let dist = (p.x - q.x).powi(2) + (p.y - q.y).powi(2);
        if dist < best.0 {
            best = (dist, q.z);
        }
    }
    best.1
}
