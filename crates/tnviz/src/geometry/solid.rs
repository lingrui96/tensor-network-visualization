//! 3D solids and the view (docs/language.md, section 11): the rounded
//! solids, their surfaces, and how they project onto the page.

use super::path::{Path, Piece, circle};
use crate::layout::{Rot, V3};
use crate::model::Scene;

fn dot(a: V3, b: V3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn cross(a: V3, b: V3) -> V3 {
    V3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}

// ---- The view --------------------------------------------------------------

/// The rotation from the world to view coordinates (section 11.2): x to the
/// right of the page, y up, z towards the viewer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The world x, y, and z axes in view coordinates.
    pub x: V3,
    pub y: V3,
    pub z: V3,
}

/// The default of section 11.2.
pub const DEFAULT_CAMERA: (f64, f64) = (-30.0, 35.0);

impl View {
    /// From two axes, made orthonormal.
    pub fn from_axes(x: V3, y: V3) -> View {
        let x = x.unit().expect("a non-zero view axis");
        let y = (y - x * dot(x, y)).unit().expect("view axes that are not parallel");
        View { x, y, z: cross(x, y) }
    }

    /// `camera a e`: azimuth about the world z axis, elevation above xy.
    pub fn camera(a: f64, e: f64) -> View {
        let (se, ce) = e.to_radians().sin_cos();
        let right = V3::new(1.0, 0.0, 0.0).rotate_z(a);
        let up = V3::new(0.0, se, ce).rotate_z(a);
        let toward = V3::new(0.0, -ce, se).rotate_z(a);
        // The view coordinates of a world axis are its components along
        // right, up, and towards.
        let axis = |w: V3| V3::new(dot(w, right), dot(w, up), dot(w, toward));
        View {
            x: axis(V3::new(1.0, 0.0, 0.0)),
            y: axis(V3::new(0.0, 1.0, 0.0)),
            z: axis(V3::new(0.0, 0.0, 1.0)),
        }
    }

    pub fn from_scene(scene: &Scene) -> View {
        let v = |a: [f64; 3]| V3::new(a[0], a[1], a[2]);
        match (&scene.view, scene.camera.as_ref().and_then(|c| c.angles)) {
            (Some((x, y)), _) => View::from_axes(v(*x), v(*y)),
            (None, Some((a, e))) => View::camera(a, e),
            (None, None) => View::camera(DEFAULT_CAMERA.0, DEFAULT_CAMERA.1),
        }
    }

    /// A world vector in view coordinates.
    pub fn apply(&self, w: V3) -> V3 {
        self.x * w.x + self.y * w.y + self.z * w.z
    }

    /// A view vector in world coordinates.
    pub fn inverse(&self, v: V3) -> V3 {
        V3::new(dot(v, self.x), dot(v, self.y), dot(v, self.z))
    }

    /// The page position of a world point.
    pub fn page(&self, w: V3) -> V3 {
        let v = self.apply(w);
        V3::xy(v.x, v.y)
    }

    /// The depth of a world point: larger is nearer.
    pub fn depth(&self, w: V3) -> f64 {
        self.apply(w).z
    }
}

// ---- Solids ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    Ball {
        radius: f64,
    },
    /// A convex polyhedron grown by `radius`: the shrunk solid of section
    /// 11.3, in the local frame, with faces wound counter-clockwise seen
    /// from outside.
    Rounded {
        verts: Vec<V3>,
        faces: Vec<Vec<usize>>,
        radius: f64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Solid {
    pub center: V3,
    pub orient: Rot,
    pub form: Form,
}

impl Form {
    pub fn ball(diameter: f64) -> Form {
        Form::Ball { radius: diameter / 2.0 }
    }

    /// A box `w` × `h` × `t` rounded by `r`.
    pub fn rounded_box(w: f64, h: f64, t: f64, r: f64) -> Form {
        let r = r.clamp(0.0, 0.98 * w.min(h).min(t) / 2.0);
        let (x, y, z) = (w / 2.0 - r, h / 2.0 - r, t / 2.0 - r);
        let verts = (0..8)
            .map(|k| {
                let s = |bit: usize| if k & bit != 0 { 1.0 } else { -1.0 };
                V3::new(x * s(1), y * s(2), z * s(4))
            })
            .collect();
        let faces = vec![
            vec![0, 2, 6, 4],
            vec![1, 5, 7, 3],
            vec![0, 4, 5, 1],
            vec![2, 3, 7, 6],
            vec![0, 1, 3, 2],
            vec![4, 6, 7, 5],
        ];
        Form::rounded(verts, faces, r)
    }

    /// The 2D triangle of `w` × `h` (apex up) extruded by `t`, rounded by `r`.
    pub fn prism(w: f64, h: f64, t: f64, r: f64) -> Form {
        let tri = [V3::xy(-w / 2.0, -h / 2.0), V3::xy(w / 2.0, -h / 2.0), V3::xy(0.0, h / 2.0)];
        // Inradius: area over half the perimeter.
        let side = (w * w / 4.0 + h * h).sqrt();
        let inradius = w * h / (w + 2.0 * side);
        let r = r.clamp(0.0, 0.98 * inradius.min(t / 2.0));
        let inner = shrink_polygon(&tri, r);
        let z = t / 2.0 - r;
        let mut verts: Vec<V3> = inner.iter().map(|p| V3::new(p.x, p.y, -z)).collect();
        verts.extend(inner.iter().map(|p| V3::new(p.x, p.y, z)));
        let faces = vec![vec![0, 1, 2], vec![3, 4, 5], vec![0, 1, 4, 3], vec![1, 2, 5, 4], vec![2, 0, 3, 5]];
        Form::rounded(verts, faces, r)
    }

    /// Vertices at (±w/2, 0, 0), (0, ±h/2, 0), (0, 0, ±t/2), rounded by `r`.
    pub fn octahedron(w: f64, h: f64, t: f64, r: f64) -> Form {
        let (a, b, c) = (w / 2.0, h / 2.0, t / 2.0);
        // Every face is at distance d from the centre; shrinking by r
        // scales the octahedron by (d - r) / d.
        let d = 1.0 / (1.0 / (a * a) + 1.0 / (b * b) + 1.0 / (c * c)).sqrt();
        let r = r.clamp(0.0, 0.98 * d);
        let s = (d - r) / d;
        let verts = vec![
            V3::new(a * s, 0.0, 0.0),
            V3::new(-a * s, 0.0, 0.0),
            V3::new(0.0, b * s, 0.0),
            V3::new(0.0, -b * s, 0.0),
            V3::new(0.0, 0.0, c * s),
            V3::new(0.0, 0.0, -c * s),
        ];
        let mut faces = Vec::new();
        for &x in &[0, 1] {
            for &y in &[2, 3] {
                for &z in &[4, 5] {
                    faces.push(vec![x, y, z]);
                }
            }
        }
        Form::rounded(verts, faces, r)
    }

    /// Wind every face counter-clockwise seen from outside.
    fn rounded(verts: Vec<V3>, mut faces: Vec<Vec<usize>>, radius: f64) -> Form {
        let centroid = |f: &[usize]| f.iter().fold(V3::ZERO, |a, &k| a + verts[k]) * (1.0 / f.len() as f64);
        for f in &mut faces {
            let n = cross(verts[f[1]] - verts[f[0]], verts[f[2]] - verts[f[0]]);
            if dot(n, centroid(f)) < 0.0 {
                f.reverse();
            }
        }
        Form::Rounded { verts, faces, radius }
    }
}

/// A convex counter-clockwise polygon with every edge moved inwards by `r`.
fn shrink_polygon(poly: &[V3], r: f64) -> Vec<V3> {
    let n = poly.len();
    let line = |k: usize| {
        let (a, b) = (poly[k], poly[(k + 1) % n]);
        let d = (b - a).unit().unwrap();
        let inward = V3::xy(-d.y, d.x);
        (a + inward * r, d)
    };
    (0..n)
        .map(|k| {
            let (p, d) = line((k + n - 1) % n);
            let (q, e) = line(k);
            // p + d s = q + e u, in 2D.
            let det = d.x * (-e.y) - d.y * (-e.x);
            let s = ((q.x - p.x) * (-e.y) - (q.y - p.y) * (-e.x)) / det;
            p + d * s
        })
        .collect()
}

impl Solid {
    fn local(&self, w: V3) -> V3 {
        self.orient.inverse().apply(w - self.center)
    }

    fn world(&self, l: V3) -> V3 {
        self.center + self.orient.apply(l)
    }

    /// Positive outside the solid, and the distance to it there.
    pub fn outside(&self, w: V3) -> f64 {
        let p = self.local(w);
        match &self.form {
            Form::Ball { radius } => p.norm() - radius,
            Form::Rounded { verts, faces, radius } => distance_to_polyhedron(p, verts, faces) - radius,
        }
    }

    /// How far a ray from inside travels before it leaves the solid.
    pub fn exit(&self, origin: V3, dir: V3) -> f64 {
        if self.outside(origin) > 0.0 {
            return 0.0;
        }
        let mut hi = 0.1;
        while self.outside(origin + dir * hi) <= 0.0 {
            hi *= 2.0;
            if hi > 1e6 {
                return 0.0;
            }
        }
        let mut lo = 0.0;
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            if self.outside(origin + dir * mid) <= 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) / 2.0
    }

    /// The outline on the page (section 11.3).
    pub fn silhouette(&self, view: &View) -> Path {
        match &self.form {
            Form::Ball { radius } => circle(view.page(self.center), *radius),
            Form::Rounded { verts, radius, .. } => {
                let pts: Vec<V3> = verts.iter().map(|v| view.page(self.world(*v))).collect();
                grown_hull(&hull(&pts), *radius)
            }
        }
    }

    /// The visible surface as patches, in drawing order (section 11.3):
    /// vertices, then edges, each from far to near, then the faces that
    /// face the viewer.  Everything is in page coordinates, and normals and
    /// axes in view coordinates.
    pub fn patches(&self, view: &View) -> Vec<Patch> {
        let Form::Rounded { verts, faces, radius } = &self.form else {
            return vec![Patch::Sphere { center: view.page(self.center), radius: self.ball_radius() }];
        };
        let r = *radius;
        let world: Vec<V3> = verts.iter().map(|v| self.world(*v)).collect();
        let normal = |f: &[usize]| {
            let n = cross(verts[f[1]] - verts[f[0]], verts[f[2]] - verts[f[0]]).unit().unwrap();
            view.apply(self.orient.apply(n))
        };
        let normals: Vec<V3> = faces.iter().map(|f| normal(f)).collect();
        let front: Vec<bool> = normals.iter().map(|n| n.z > 1e-9).collect();

        // Every edge with the faces on either side of it.
        let mut edges: Vec<(usize, usize, Vec<usize>)> = Vec::new();
        for (fi, f) in faces.iter().enumerate() {
            for k in 0..f.len() {
                let (a, b) = (f[k].min(f[(k + 1) % f.len()]), f[k].max(f[(k + 1) % f.len()]));
                match edges.iter_mut().find(|e| e.0 == a && e.1 == b) {
                    Some(e) => e.2.push(fi),
                    None => edges.push((a, b, vec![fi])),
                }
            }
        }
        let mut shown_vert = vec![false; verts.len()];
        for (fi, f) in faces.iter().enumerate() {
            if front[fi] {
                for &k in f {
                    shown_vert[k] = true;
                }
            }
        }
        let depth = |w: V3| view.depth(w);
        let mut out = Vec::new();
        let mut vs: Vec<usize> = (0..verts.len()).filter(|&k| shown_vert[k]).collect();
        vs.sort_by(|&a, &b| depth(world[a]).total_cmp(&depth(world[b])));
        out.extend(vs.into_iter().map(|k| Patch::Sphere { center: view.page(world[k]), radius: r }));
        let mut es: Vec<&(usize, usize, Vec<usize>)> =
            edges.iter().filter(|e| e.2.iter().any(|&f| front[f])).collect();
        es.sort_by(|a, b| depth(world[a.0] + world[a.1]).total_cmp(&depth(world[b.0] + world[b.1])));
        for (a, b, fs) in es {
            let axis = view.apply(world[*b] - world[*a]);
            let (pa, pb) = (view.page(world[*a]), view.page(world[*b]));
            if (pb - pa).norm() < 1e-9 || fs.len() != 2 {
                continue;
            }
            let axis = axis.unit().unwrap();
            // The edge's surface is the arc of its cylinder between the
            // normals of its two faces; only the part facing the viewer
            // shows.  Angles are about the axis, from e (across it on the
            // page) towards w (towards the viewer).
            let d = (pb - pa).unit().unwrap();
            let e = V3::xy(-d.y, d.x);
            let w = edge_toward(axis, e);
            let angle = |n: V3| dot(n, w).atan2(dot(n, e));
            let (mut t1, mut t2) = (angle(normals[fs[0]]), angle(normals[fs[1]]));
            if t2 < t1 {
                std::mem::swap(&mut t1, &mut t2);
            }
            // The arc between them is the shorter one, less than π.
            let (from, to) =
                if t2 - t1 <= std::f64::consts::PI { (t1, t2) } else { (t2, t1 + std::f64::consts::TAU) };
            // Intersect with [0, π], the half facing the viewer.
            let (from, to) = clip_facing(from, to);
            if to > from + 1e-9 {
                out.push(Patch::Edge { a: pa, b: pb, axis, radius: r, from, to });
            }
        }
        for (fi, f) in faces.iter().enumerate() {
            if front[fi] {
                let lift = self
                    .orient
                    .apply(cross(verts[f[1]] - verts[f[0]], verts[f[2]] - verts[f[0]]).unit().unwrap())
                    * r;
                let pts: Vec<V3> = f.iter().map(|&k| view.page(world[k] + lift)).collect();
                out.push(Patch::Face { outline: polygon(&pts), normal: normals[fi] });
            }
        }
        out
    }

    /// The radius of a ball about the centre that holds the solid.
    pub fn reach(&self) -> f64 {
        match &self.form {
            Form::Ball { radius } => *radius,
            Form::Rounded { verts, radius, .. } => {
                verts.iter().map(|v| v.norm()).fold(0.0, f64::max) + radius
            }
        }
    }

    /// The depth of the visible surface at a page point, if the solid
    /// covers it.
    pub fn surface_depth(&self, view: &View, p: V3) -> Option<f64> {
        let (c, reach) = (view.apply(self.center), self.reach());
        if let Form::Ball { radius } = self.form {
            let d2 = (p.x - c.x).powi(2) + (p.y - c.y).powi(2);
            return (d2 <= radius * radius).then(|| c.z + (radius * radius - d2).sqrt());
        }
        let at = |z: f64| view.inverse(V3::new(p.x, p.y, z));
        let (top, steps) = (c.z + reach + 1e-6, 32);
        let mut outside = top;
        for i in 1..=steps {
            let z = top - 2.0 * reach * i as f64 / steps as f64;
            if self.outside(at(z)) <= 0.0 {
                let (mut lo, mut hi) = (z, outside);
                for _ in 0..40 {
                    let mid = (lo + hi) / 2.0;
                    if self.outside(at(mid)) <= 0.0 {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                return Some((lo + hi) / 2.0);
            }
            outside = z;
        }
        None
    }

    fn ball_radius(&self) -> f64 {
        match &self.form {
            Form::Ball { radius } => *radius,
            Form::Rounded { radius, .. } => *radius,
        }
    }
}

/// The part of the angle range [from, to] (to − from < 2π) within [0, π].
fn clip_facing(from: f64, to: f64) -> (f64, f64) {
    let tau = std::f64::consts::TAU;
    // Shift so that `from` is in [0, 2π).
    let k = (from / tau).floor();
    let (f, t) = (from - k * tau, to - k * tau);
    let (lo, hi) = (f.max(0.0), t.min(std::f64::consts::PI));
    if hi > lo {
        return (lo, hi);
    }
    // The range may wrap past 2π into the next [0, π].
    let (lo, hi) = (f.max(tau), t.min(tau + std::f64::consts::PI));
    if hi > lo { (lo - tau, hi - tau) } else { (0.0, 0.0) }
}

/// The unit vector across a cylinder of axis `axis` (view coordinates),
/// perpendicular to it and to the page direction `e`, towards the viewer.
pub fn edge_toward(axis: V3, e: V3) -> V3 {
    let w = cross(axis, e).unit().unwrap_or(V3::new(0.0, 0.0, 1.0));
    if w.z < 0.0 { -w } else { w }
}

/// A piece of a solid's visible surface.
#[derive(Clone, Debug, PartialEq)]
pub enum Patch {
    /// A sphere, or a rounded vertex: a disc on the page.
    Sphere { center: V3, radius: f64 },
    /// A rounded edge: the part of a cylinder about the segment from `a` to
    /// `b` on the page between the angles `from` and `to` about its axis (in
    /// view coordinates), measured from the page direction across it
    /// towards the viewer, within [0, π].
    Edge { a: V3, b: V3, axis: V3, radius: f64, from: f64, to: f64 },
    /// A flat face and its normal in view coordinates.
    Face { outline: Path, normal: V3 },
}

impl Patch {
    /// The region the patch covers on the page.
    pub fn region(&self) -> Path {
        match self {
            Patch::Sphere { center, radius } => circle(*center, *radius),
            Patch::Edge { a, b, axis, radius, from, to } => {
                // At angle θ, the surface is r (cos θ e + sin θ w) from the
                // axis; on the page w shortens to its page part, so each end
                // is an arc of an ellipse, the end circle seen from the
                // viewer (section 11.3).
                let d = (*b - *a).unit().unwrap();
                let e = V3::xy(-d.y, d.x);
                let w = edge_toward(*axis, e);
                let wp = V3::xy(w.x, w.y);
                let r = *radius;
                let end = |c: V3, t0: f64, t1: f64| -> Vec<V3> {
                    (0..=16)
                        .map(|i| {
                            let t = t0 + (t1 - t0) * i as f64 / 16.0;
                            c + e * (r * t.cos()) + wp * (r * t.sin())
                        })
                        .collect()
                };
                let mut pts = end(*a, *from, *to);
                pts.extend(end(*b, *to, *from));
                polygon(&pts)
            }
            Patch::Face { outline, .. } => outline.clone(),
        }
    }
}

/// The part of a polygon inside a convex counter-clockwise polygon.
pub(crate) fn clip_convex(subject: &[V3], clip: &[V3]) -> Vec<V3> {
    let inside = |p: V3, a: V3, b: V3| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x) >= -1e-12;
    let cut = |p: V3, q: V3, a: V3, b: V3| {
        let (d, e) = (q - p, b - a);
        let den = d.x * e.y - d.y * e.x;
        if den.abs() < 1e-15 {
            return p;
        }
        let t = ((a.x - p.x) * e.y - (a.y - p.y) * e.x) / den;
        p + d * t
    };
    let mut out = subject.to_vec();
    for k in 0..clip.len() {
        let (a, b) = (clip[k], clip[(k + 1) % clip.len()]);
        let input = std::mem::take(&mut out);
        for i in 0..input.len() {
            let (p, q) = (input[i], input[(i + 1) % input.len()]);
            match (inside(p, a, b), inside(q, a, b)) {
                (true, true) => out.push(q),
                (true, false) => out.push(cut(p, q, a, b)),
                (false, true) => {
                    out.push(cut(p, q, a, b));
                    out.push(q);
                }
                (false, false) => {}
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

/// The runs of an open polyline inside a convex counter-clockwise polygon.
pub(crate) fn clip_polyline_convex(line: &[V3], clip: &[V3]) -> Vec<Vec<V3>> {
    let mut runs: Vec<Vec<V3>> = Vec::new();
    let mut open = false;
    for w in line.windows(2) {
        // Cyrus–Beck: the part of the segment within every edge.
        let (p, d) = (w[0], w[1] - w[0]);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        for k in 0..clip.len() {
            let (a, b) = (clip[k], clip[(k + 1) % clip.len()]);
            let n = V3::xy(-(b.y - a.y), b.x - a.x);
            let (num, den) = ((p.x - a.x) * n.x + (p.y - a.y) * n.y, d.x * n.x + d.y * n.y);
            if den.abs() < 1e-15 {
                if num < 0.0 {
                    t1 = -1.0;
                }
                continue;
            }
            let t = -num / den;
            if den > 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
        if t1 > t0 + 1e-12 {
            let (a, b) = (p + d * t0, p + d * t1);
            match runs.last_mut() {
                Some(run) if open && t0 < 1e-9 => run.push(b),
                _ => runs.push(vec![a, b]),
            }
            open = t1 > 1.0 - 1e-9;
        } else {
            open = false;
        }
    }
    runs
}

fn polygon(pts: &[V3]) -> Path {
    let n = pts.len();
    Path { pieces: (0..n).map(|k| Piece::Line { a: pts[k], b: pts[(k + 1) % n] }).collect(), closed: true }
}

/// The convex hull of page points, counter-clockwise, without repeats.
pub(crate) fn hull(pts: &[V3]) -> Vec<V3> {
    let mut p: Vec<V3> = pts.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup_by(|a, b| (*a - *b).norm() < 1e-9);
    if p.len() < 3 {
        return p;
    }
    let turn = |o: V3, a: V3, b: V3| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut lower: Vec<V3> = Vec::new();
    for &q in &p {
        while lower.len() >= 2 && turn(lower[lower.len() - 2], lower[lower.len() - 1], q) <= 1e-12 {
            lower.pop();
        }
        lower.push(q);
    }
    let mut upper: Vec<V3> = Vec::new();
    for &q in p.iter().rev() {
        while upper.len() >= 2 && turn(upper[upper.len() - 2], upper[upper.len() - 1], q) <= 1e-12 {
            upper.pop();
        }
        upper.push(q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// A convex counter-clockwise polygon grown by `r`: its edges moved out by
/// `r` and joined by arcs about its vertices.
fn grown_hull(h: &[V3], r: f64) -> Path {
    match h.len() {
        0 => return Path { pieces: Vec::new(), closed: true },
        1 => return circle(h[0], r),
        _ => {}
    }
    let n = h.len();
    let out_normal = |k: usize| {
        let d = (h[(k + 1) % n] - h[k]).unit().unwrap();
        V3::xy(d.y, -d.x)
    };
    let angle = |v: V3| v.y.atan2(v.x);
    let mut pieces = Vec::new();
    for k in 0..n {
        let (n_in, n_out) = (out_normal((k + n - 1) % n), out_normal(k));
        let start = angle(n_in);
        let mut sweep = angle(n_out) - start;
        while sweep < 0.0 {
            sweep += std::f64::consts::TAU;
        }
        if r > 1e-12 && sweep > 1e-12 {
            pieces.push(Piece::Arc { center: h[k], radius: r, start, sweep });
        }
        pieces.push(Piece::Line { a: h[k] + n_out * r, b: h[(k + 1) % n] + n_out * r });
    }
    Path { pieces, closed: true }
}

/// The distance from a point to a convex polyhedron, 0 inside.
fn distance_to_polyhedron(p: V3, verts: &[V3], faces: &[Vec<usize>]) -> f64 {
    let mut inside = true;
    let mut best = f64::INFINITY;
    for f in faces {
        let (a, b, c) = (verts[f[0]], verts[f[1]], verts[f[2]]);
        let n = match cross(b - a, c - a).unit() {
            Some(n) => n,
            None => continue,
        };
        let h = dot(p - a, n);
        if h > 1e-12 {
            inside = false;
        }
        // The nearest point of the face: in it, or on one of its edges.
        let q = p - n * h;
        let within = (0..f.len()).all(|k| {
            let (u, v) = (verts[f[k]], verts[f[(k + 1) % f.len()]]);
            dot(cross(v - u, q - u), n) >= -1e-12
        });
        let d = if within {
            h.abs()
        } else {
            (0..f.len())
                .map(|k| segment_distance(p, verts[f[k]], verts[f[(k + 1) % f.len()]]))
                .fold(f64::INFINITY, f64::min)
        };
        best = best.min(d);
    }
    if inside { 0.0 } else { best }
}

fn segment_distance(p: V3, a: V3, b: V3) -> f64 {
    let d = b - a;
    let len2 = dot(d, d);
    let t = if len2 > 0.0 { (dot(p - a, d) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + d * t)).norm()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn views() {
        // `camera 0 90` looks down z and shows the 2D picture.
        let v = View::camera(0.0, 90.0);
        assert!((v.page(V3::new(2.0, 3.0, 5.0)) - V3::xy(2.0, 3.0)).norm() < 1e-9);
        assert!(close(v.depth(V3::new(0.0, 0.0, 1.0)), 1.0));
        // Right-handed, and orthonormalised from two axes.
        let v = View::from_axes(V3::new(2.0, 0.0, 0.0), V3::new(1.0, 1.0, 0.0));
        assert!((v.y - V3::new(0.0, 1.0, 0.0)).norm() < 1e-9 && (v.z - V3::new(0.0, 0.0, 1.0)).norm() < 1e-9);
        let v = View::camera(-30.0, 35.0);
        assert!((cross(v.x, v.y) - v.z).norm() < 1e-9);
        assert!((v.inverse(v.apply(V3::new(1.0, 2.0, 3.0))) - V3::new(1.0, 2.0, 3.0)).norm() < 1e-9);
    }

    #[test]
    fn rounded_solids_keep_their_faces() {
        let s = |form| Solid { center: V3::ZERO, orient: Rot::IDENTITY, form };
        // A box's faces stay where the sharp box's are.
        let b = s(Form::rounded_box(2.0, 1.0, 1.0, 0.2));
        assert!(close(b.exit(V3::ZERO, V3::new(1.0, 0.0, 0.0)), 1.0));
        assert!(close(b.exit(V3::ZERO, V3::new(0.0, 0.0, 1.0)), 0.5));
        // A corner is rounded: the diagonal is shorter than the sharp one.
        let diag = V3::new(1.0, 0.5, 0.5).unit().unwrap();
        let sharp = V3::new(1.0, 0.5, 0.5).norm();
        assert!(b.exit(V3::ZERO, diag) < sharp - 0.1);
        // The octahedron's vertex along x, rounded.
        let o = s(Form::octahedron(2.0, 2.0, 2.0, 0.0));
        assert!(close(o.exit(V3::ZERO, V3::new(1.0, 0.0, 0.0)), 1.0));
        let face = V3::new(1.0, 1.0, 1.0).unit().unwrap();
        assert!(close(o.exit(V3::ZERO, face), 1.0 / 3f64.sqrt()));
        let p = s(Form::prism(1.0, 1.0, 1.0, 0.1));
        assert!(close(p.exit(V3::ZERO, V3::new(0.0, 0.0, 1.0)), 0.5));
        assert!(close(p.exit(V3::ZERO, V3::new(0.0, -1.0, 0.0)), 0.5));
    }

    #[test]
    fn silhouettes_from_the_front_are_the_2d_shapes() {
        let front = View::camera(0.0, 90.0);
        let b =
            Solid { center: V3::ZERO, orient: Rot::IDENTITY, form: Form::rounded_box(2.0, 1.0, 1.0, 0.2) };
        let out = b.silhouette(&front);
        assert!(close(out.ray_distance(V3::ZERO, V3::xy(1.0, 0.0)).unwrap(), 1.0));
        assert!(close(out.ray_distance(V3::ZERO, V3::xy(0.0, 1.0)).unwrap(), 0.5));
        // Seen from the front, a box shows one face and no edges.
        let faces = b.patches(&front).iter().filter(|p| matches!(p, Patch::Face { .. })).count();
        assert_eq!(faces, 1);
        // From the default camera, three faces.
        let cam = View::camera(-30.0, 35.0);
        let faces = b.patches(&cam).iter().filter(|p| matches!(p, Patch::Face { .. })).count();
        assert_eq!(faces, 3);
    }
}
