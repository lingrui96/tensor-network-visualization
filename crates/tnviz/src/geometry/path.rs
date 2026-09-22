//! Paths of straight segments and circular arcs: rounded outlines, filleted
//! centrelines, tube outlines, arc length, and intersections.

use std::f64::consts::{PI, TAU};

use crate::layout::V3;

const EPS: f64 = 1e-9;

fn cross(a: V3, b: V3) -> f64 {
    a.x * b.y - a.y * b.x
}

fn dot(a: V3, b: V3) -> f64 {
    a.x * b.x + a.y * b.y
}

/// The left normal of a direction.
fn left(d: V3) -> V3 {
    V3::xy(-d.y, d.x)
}

fn polar(angle: f64) -> V3 {
    V3::xy(angle.cos(), angle.sin())
}

/// A straight segment, or a circular arc from angle `start` sweeping by
/// `sweep` radians (positive counter-clockwise).
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Line { a: V3, b: V3 },
    Arc { center: V3, radius: f64, start: f64, sweep: f64 },
}

impl Piece {
    pub fn length(&self) -> f64 {
        match self {
            Piece::Line { a, b } => (*b - *a).norm(),
            Piece::Arc { radius, sweep, .. } => radius * sweep.abs(),
        }
    }

    pub fn start(&self) -> V3 {
        self.at(0.0).0
    }

    pub fn end(&self) -> V3 {
        self.at(self.length()).0
    }

    /// The point and unit tangent at arc length `s` from the start.
    pub fn at(&self, s: f64) -> (V3, V3) {
        match *self {
            Piece::Line { a, b } => {
                let d = (b - a).unit().unwrap_or(V3::xy(1.0, 0.0));
                (a + d * s, d)
            }
            Piece::Arc { center, radius, start, sweep } => {
                let len = radius * sweep.abs();
                let t = if len > EPS { s / len } else { 0.0 };
                let angle = start + sweep * t;
                (center + polar(angle) * radius, left(polar(angle)) * sweep.signum())
            }
        }
    }

    fn reversed(&self) -> Piece {
        match *self {
            Piece::Line { a, b } => Piece::Line { a: b, b: a },
            Piece::Arc { center, radius, start, sweep } => {
                Piece::Arc { center, radius, start: start + sweep, sweep: -sweep }
            }
        }
    }

    fn transformed(&self, rotation: f64, shift: V3) -> Piece {
        let f = |p: V3| p.rotate_z(rotation) + shift;
        match *self {
            Piece::Line { a, b } => Piece::Line { a: f(a), b: f(b) },
            Piece::Arc { center, radius, start, sweep } => {
                Piece::Arc { center: f(center), radius, start: start + rotation.to_radians(), sweep }
            }
        }
    }

    /// Arc length along an arc to the point at `angle`, if the arc covers it.
    fn arc_param(start: f64, sweep: f64, radius: f64, angle: f64) -> Option<f64> {
        let d = if sweep >= 0.0 { (angle - start).rem_euclid(TAU) } else { (start - angle).rem_euclid(TAU) };
        let d = if d > TAU - 1e-9 { 0.0 } else { d };
        (d <= sweep.abs() + 1e-9).then_some(d * radius)
    }

    /// Intersections with another piece: points with the arc length along
    /// `self` and along `other`.
    pub fn intersect(&self, other: &Piece) -> Vec<(V3, f64, f64)> {
        match (self, other) {
            (Piece::Line { a, b }, Piece::Line { a: c, b: d }) => {
                let (r, s) = (*b - *a, *d - *c);
                let den = cross(r, s);
                if den.abs() < EPS * r.norm() * s.norm() {
                    return Vec::new();
                }
                let q = *c - *a;
                let (t, u) = (cross(q, s) / den, cross(q, r) / den);
                if (-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u) {
                    vec![(*a + r * t, t * r.norm(), u * s.norm())]
                } else {
                    Vec::new()
                }
            }
            (Piece::Line { a, b }, Piece::Arc { center, radius, start, sweep }) => {
                let d = *b - *a;
                let f = *a - *center;
                let (qa, qb, qc) = (dot(d, d), 2.0 * dot(f, d), dot(f, f) - radius * radius);
                let disc = qb * qb - 4.0 * qa * qc;
                if qa < EPS || disc < 0.0 {
                    return Vec::new();
                }
                let root = disc.sqrt();
                let mut out = Vec::new();
                for t in [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)] {
                    if !(-1e-9..=1.0 + 1e-9).contains(&t) {
                        continue;
                    }
                    let p = *a + d * t;
                    let angle = (p.y - center.y).atan2(p.x - center.x);
                    if let Some(u) = Self::arc_param(*start, *sweep, *radius, angle)
                        && !out.iter().any(|(q, _, _): &(V3, f64, f64)| (*q - p).norm() < 1e-9)
                    {
                        out.push((p, t * d.norm(), u));
                    }
                }
                out
            }
            (Piece::Arc { .. }, Piece::Line { .. }) => {
                other.intersect(self).into_iter().map(|(p, s, t)| (p, t, s)).collect()
            }
            (
                Piece::Arc { center: c1, radius: r1, start: s1, sweep: w1 },
                Piece::Arc { center: c2, radius: r2, start: s2, sweep: w2 },
            ) => {
                let d = *c2 - *c1;
                let dist = d.norm();
                if dist < EPS || dist > r1 + r2 || dist < (r1 - r2).abs() {
                    return Vec::new();
                }
                let a = (r1 * r1 - r2 * r2 + dist * dist) / (2.0 * dist);
                let h = (r1 * r1 - a * a).max(0.0).sqrt();
                let base = *c1 + d * (a / dist);
                let mut out: Vec<(V3, f64, f64)> = Vec::new();
                for p in [base + left(d) * (h / dist), base - left(d) * (h / dist)] {
                    let a1 = (p.y - c1.y).atan2(p.x - c1.x);
                    let a2 = (p.y - c2.y).atan2(p.x - c2.x);
                    if let (Some(u), Some(v)) =
                        (Self::arc_param(*s1, *w1, *r1, a1), Self::arc_param(*s2, *w2, *r2, a2))
                        && !out.iter().any(|(q, _, _)| (*q - p).norm() < 1e-9)
                    {
                        out.push((p, u, v));
                    }
                }
                out
            }
        }
    }
}

/// A sequence of pieces, each starting where the previous one ends.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Path {
    pub pieces: Vec<Piece>,
    pub closed: bool,
}

impl Path {
    pub fn length(&self) -> f64 {
        self.pieces.iter().map(Piece::length).sum()
    }

    /// The point and unit tangent at arc length `s`, clamped to the path.
    pub fn at(&self, s: f64) -> (V3, V3) {
        let mut rest = s.max(0.0);
        for (k, p) in self.pieces.iter().enumerate() {
            let len = p.length();
            if rest <= len || k + 1 == self.pieces.len() {
                return p.at(rest.min(len));
            }
            rest -= len;
        }
        (V3::ZERO, V3::xy(1.0, 0.0))
    }

    /// Points along the path, with arcs split into chords of at most `step`
    /// radians.
    pub fn sample(&self, step: f64) -> Vec<V3> {
        let mut out = Vec::new();
        for p in &self.pieces {
            match *p {
                Piece::Line { a, b } => out.extend([a, b]),
                Piece::Arc { sweep, .. } => {
                    let n = ((sweep.abs() / step).ceil() as usize).max(1);
                    let len = p.length();
                    out.extend((0..=n).map(|k| p.at(len * k as f64 / n as f64).0));
                }
            }
        }
        out.dedup_by(|a, b| (*a - *b).norm() < 1e-12);
        out
    }

    pub fn transformed(&self, rotation: f64, shift: V3) -> Path {
        Path {
            pieces: self.pieces.iter().map(|p| p.transformed(rotation, shift)).collect(),
            closed: self.closed,
        }
    }

    /// The nearest point where a ray from `origin` along the unit vector
    /// `dir` meets the path, as a distance.
    pub fn ray_distance(&self, origin: V3, dir: V3) -> Option<f64> {
        let far = self.pieces.iter().map(|p| (p.start() - origin).norm()).fold(0.0, f64::max) * 4.0 + 1.0;
        let ray = Piece::Line { a: origin, b: origin + dir * far };
        self.pieces
            .iter()
            .flat_map(|p| ray.intersect(p))
            .map(|(_, s, _)| s)
            .filter(|s| *s > 1e-9)
            .reduce(f64::min)
    }
}

/// A corner of a polyline, with the largest fillet radius it may get and
/// the fraction of each adjacent segment the fillet may use.
#[derive(Clone, Debug, PartialEq)]
pub struct Vertex {
    pub p: V3,
    pub radius: f64,
    pub clamp: f64,
}

/// The result of filleting a polyline.
#[derive(Clone, Debug, Default)]
pub struct Filleted {
    pub path: Path,
    /// For each piece, the polyline segment it lies on, or `None` for a
    /// fillet.
    pub segment: Vec<Option<usize>>,
    /// The radius each interior vertex got.
    pub radii: Vec<f64>,
}

/// Round the corners of an open polyline.  Vertex `i` gets the radius
/// min(its radius, clamp · min(adjacent lengths) / tan(|δ|/2)); corners
/// that turn by less than half a degree stay sharp.
pub fn fillet_open(vs: &[Vertex]) -> Filleted {
    let n = vs.len();
    let mut out = Filleted::default();
    if n < 2 {
        return out;
    }
    let dirs: Vec<V3> =
        (0..n - 1).map(|k| (vs[k + 1].p - vs[k].p).unit().unwrap_or(V3::xy(1.0, 0.0))).collect();
    let lens: Vec<f64> = (0..n - 1).map(|k| (vs[k + 1].p - vs[k].p).norm()).collect();
    // Tangent length and arc at every vertex; ends have none.
    let mut tl = vec![0.0; n];
    let mut arcs: Vec<Option<Piece>> = vec![None; n];
    for i in 1..n - 1 {
        let (e0, e1) = (dirs[i - 1], dirs[i]);
        let delta = cross(e0, e1).atan2(dot(e0, e1));
        if delta.abs() < 0.5f64.to_radians() {
            out.radii.push(0.0);
            continue;
        }
        let tan = (delta.abs() / 2.0).tan();
        let rho = vs[i].radius.min(vs[i].clamp * lens[i - 1].min(lens[i]) / tan).max(0.0);
        out.radii.push(rho);
        if rho <= EPS {
            continue;
        }
        tl[i] = rho * tan;
        let t_in = vs[i].p - e0 * tl[i];
        let center = t_in + left(e0) * (delta.signum() * rho);
        let start = (t_in.y - center.y).atan2(t_in.x - center.x);
        arcs[i] = Some(Piece::Arc { center, radius: rho, start, sweep: delta });
    }
    for k in 0..n - 1 {
        let a = vs[k].p + dirs[k] * tl[k];
        let b = vs[k + 1].p - dirs[k] * tl[k + 1];
        if (b - a).norm() > 1e-12 {
            out.path.pieces.push(Piece::Line { a, b });
            out.segment.push(Some(k));
        }
        if let Some(arc) = arcs[k + 1].take() {
            out.path.pieces.push(arc);
            out.segment.push(None);
        }
    }
    out
}

/// Round every corner of a closed polygon given counter-clockwise with one
/// radius, reduced so that no fillet uses more than half of either edge
/// next to it.  Returns the outline and the radius used.
pub fn fillet_closed(corners: &[V3], radius: f64) -> (Path, f64) {
    let n = corners.len();
    let dir = |k: usize| (corners[(k + 1) % n] - corners[k]).unit().unwrap();
    let len = |k: usize| (corners[(k + 1) % n] - corners[k]).norm();
    let turn = |i: usize| {
        let (e0, e1) = (dir((i + n - 1) % n), dir(i));
        cross(e0, e1).atan2(dot(e0, e1))
    };
    let used = (0..n)
        .map(|i| 0.5 * len((i + n - 1) % n).min(len(i)) / (turn(i).abs() / 2.0).tan())
        .fold(radius, f64::min)
        .max(0.0);
    let mut tl = vec![0.0; n];
    let mut arcs: Vec<Option<Piece>> = vec![None; n];
    for i in 0..n {
        let h = (i + n - 1) % n;
        let (e0, delta) = (dir(h), turn(i));
        let rho = used;
        if rho <= EPS {
            continue;
        }
        tl[i] = rho * (delta.abs() / 2.0).tan();
        let t_in = corners[i] - e0 * tl[i];
        let center = t_in + left(e0) * rho;
        let start = (t_in.y - center.y).atan2(t_in.x - center.x);
        arcs[i] = Some(Piece::Arc { center, radius: rho, start, sweep: delta });
    }
    let mut path = Path { pieces: Vec::new(), closed: true };
    for i in 0..n {
        if let Some(arc) = arcs[i].take() {
            path.pieces.push(arc);
        }
        let a = corners[i] + dir(i) * tl[i];
        let b = corners[(i + 1) % n] - dir(i) * tl[(i + 1) % n];
        if (b - a).norm() > 1e-12 {
            path.pieces.push(Piece::Line { a, b });
        }
    }
    (path, used)
}

/// A full circle.
pub fn circle(center: V3, radius: f64) -> Path {
    Path { pieces: vec![Piece::Arc { center, radius, start: 0.0, sweep: TAU }], closed: true }
}

/// How a tube ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Round,
    Flat,
}

/// The outline of the points within `r` of an open centreline: its left
/// side forward, the end cap, its right side back, and the start cap.
/// Fillets on the inner side shrink to a point when tighter than `r`.
pub fn tube_outline(center: &Path, r: f64, caps: (Cap, Cap)) -> Path {
    let offset = |p: &Piece, side: f64| -> Piece {
        match *p {
            Piece::Line { a, b } => {
                let n = left((b - a).unit().unwrap_or(V3::xy(1.0, 0.0))) * (side * r);
                Piece::Line { a: a + n, b: b + n }
            }
            Piece::Arc { center, radius, start, sweep } => {
                // Left of a counter-clockwise arc is towards its centre.
                let radius = (radius - side * sweep.signum() * r).max(0.0);
                Piece::Arc { center, radius, start, sweep }
            }
        }
    };
    let mut pieces: Vec<Piece> = center.pieces.iter().map(|p| offset(p, 1.0)).collect();
    let (end, end_dir) = center.at(center.length());
    let (start, start_dir) = center.at(0.0);
    let cap = |p: V3, d: V3, from: V3, to: V3, kind: Cap| match kind {
        Cap::Round => {
            let n = left(d);
            Piece::Arc { center: p, radius: r, start: n.y.atan2(n.x), sweep: -PI }
        }
        Cap::Flat => Piece::Line { a: from, b: to },
    };
    pieces.push(cap(end, end_dir, end + left(end_dir) * r, end - left(end_dir) * r, caps.1));
    pieces.extend(center.pieces.iter().rev().map(|p| offset(p, -1.0).reversed()));
    pieces.push(cap(start, -start_dir, start - left(start_dir) * r, start + left(start_dir) * r, caps.0));
    Path { pieces, closed: true }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn fillet_of_a_right_angle() {
        let v = |x, y| Vertex { p: V3::xy(x, y), radius: 1.0, clamp: 0.5 };
        let f = fillet_open(&[v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0)]);
        assert_eq!(f.radii, vec![1.0]);
        assert_eq!(f.path.pieces.len(), 3);
        assert!(close(f.path.length(), 3.0 + 3.0 + PI / 2.0));
        // Pieces meet end to start.
        for w in f.path.pieces.windows(2) {
            assert!((w[0].end() - w[1].start()).norm() < 1e-9);
        }
        // Clamped by short segments.
        let f = fillet_open(&[v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0)]);
        assert!(close(f.radii[0], 0.5));
    }

    #[test]
    fn rounded_square_and_rays() {
        let corners = [V3::xy(-1.0, -1.0), V3::xy(1.0, -1.0), V3::xy(1.0, 1.0), V3::xy(-1.0, 1.0)];
        let (path, used) = fillet_closed(&corners, 5.0);
        assert!(close(used, 1.0));
        // Radius 1 on a 2x2 square is a circle of radius 1.
        assert!(close(path.length(), TAU));
        let d = path.ray_distance(V3::ZERO, V3::xy(1.0, 1.0).unit().unwrap()).unwrap();
        assert!(close(d, 1.0));
        let (sharp, _) = fillet_closed(&corners, 0.0);
        let d = sharp.ray_distance(V3::ZERO, V3::xy(1.0, 1.0).unit().unwrap()).unwrap();
        assert!(close(d, 2f64.sqrt()));
    }

    #[test]
    fn intersections() {
        let l1 = Piece::Line { a: V3::xy(-2.0, 0.0), b: V3::xy(2.0, 0.0) };
        let l2 = Piece::Line { a: V3::xy(0.0, -2.0), b: V3::xy(0.0, 2.0) };
        let hits = l1.intersect(&l2);
        assert_eq!(hits.len(), 1);
        assert!(close(hits[0].1, 2.0) && close(hits[0].2, 2.0));
        let half = Piece::Arc { center: V3::ZERO, radius: 1.0, start: 0.0, sweep: PI };
        assert_eq!(l2.intersect(&half).len(), 1);
        assert_eq!(l1.intersect(&half).len(), 2);
        let other = Piece::Arc { center: V3::xy(1.0, 0.0), radius: 1.0, start: 0.0, sweep: TAU };
        assert_eq!(half.intersect(&other).len(), 1);
    }

    #[test]
    fn tube_outline_is_closed() {
        let v = |x, y| Vertex { p: V3::xy(x, y), radius: 1.0, clamp: 0.5 };
        let c = fillet_open(&[v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0)]).path;
        for caps in [(Cap::Round, Cap::Round), (Cap::Flat, Cap::Flat)] {
            let o = tube_outline(&c, 0.25, caps);
            let n = o.pieces.len();
            for k in 0..n {
                let gap = (o.pieces[k].end() - o.pieces[(k + 1) % n].start()).norm();
                assert!(gap < 1e-9, "gap {gap} after piece {k}");
            }
        }
    }
}
