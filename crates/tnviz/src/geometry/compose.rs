//! Which surface is in front, in 3D (docs/language.md, section 11.7).
//!
//! Every drawn object is a surface: a region of the page and the depth of
//! its front surface at each point of it.  Nothing here depends on the
//! shapes: objects are drawn once each, in an order, and one drawn later has
//! a hole wherever one drawn earlier is nearer.  Planes come last, with holes
//! wherever anything is nearer than them.

use super::path::Path;
use super::solid::{Solid, View, edge_toward};
use crate::layout::V3;

/// The depth of an object's front surface at page points.
#[derive(Clone, Debug)]
pub(super) enum Depth {
    Solid(Solid),
    /// A tube, or a line, of radius `r` about a polyline in view
    /// coordinates, with round ends.
    Tube {
        pts: Vec<V3>,
        r: f64,
    },
    /// A plane through `c` with normal `n`, in view coordinates.
    Plane {
        c: V3,
        n: V3,
    },
}

impl Depth {
    fn at(&self, view: &View, p: V3) -> Option<f64> {
        self.within(view, p, 0.0)
    }

    /// The depth of the front surface at the page point `p`.
    pub fn at_point(&self, view: &View, p: V3) -> Option<f64> {
        self.at(view, p)
    }

    /// The depth at `p`, or, up to `g` outside the edge, near the edge.
    fn within(&self, view: &View, p: V3, g: f64) -> Option<f64> {
        match self {
            Depth::Solid(s) => s.surface_depth(view, p).or_else(|| {
                let c = view.apply(s.center);
                let off = V3::xy(p.x - c.x, p.y - c.y);
                let d = off.norm();
                (g > 0.0 && d > g).then(|| s.surface_depth(view, p - off * (g / d))).flatten()
            }),
            Depth::Tube { pts, r } => {
                let r = *r;
                let mut best: Option<f64> = None;
                let mut near = |z: f64| best = Some(best.map_or(z, |b: f64| b.max(z)));
                // The round ends and joints.
                for q in pts {
                    let d2 = (p.x - q.x).powi(2) + (p.y - q.y).powi(2);
                    if d2 <= (r + g).powi(2) {
                        near(q.z + (r * r - d2).max(0.0).sqrt());
                    }
                }
                for w in pts.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let d = V3::xy(b.x - a.x, b.y - a.y);
                    let len2 = d.x * d.x + d.y * d.y;
                    let Some(axis) = (b - a).unit() else { continue };
                    if len2 < 1e-18 {
                        continue;
                    }
                    let t = ((p.x - a.x) * d.x + (p.y - a.y) * d.y) / len2;
                    let foot = V3::xy(a.x + d.x * t, a.y + d.y * t);
                    let dist = ((p.x - foot.x).powi(2) + (p.y - foot.y).powi(2)).sqrt();
                    if dist > r + g {
                        continue;
                    }
                    let dist = dist.min(r);
                    // The front of the wall, `r` from the axis towards the
                    // viewer, lies off the foot along the axis.
                    let s = (1.0 - (dist / r).powi(2)).max(0.0).sqrt();
                    let wall = edge_toward(axis, V3::xy(-d.y, d.x).unit().unwrap());
                    let tau = t - r * s * (wall.x * d.x + wall.y * d.y) / len2;
                    if (0.0..=1.0).contains(&tau) {
                        near(a.z + (b.z - a.z) * tau + r * s * wall.z);
                    }
                }
                best
            }
            Depth::Plane { c, n } => {
                (n.z.abs() > 1e-9).then(|| c.z - (n.x * (p.x - c.x) + n.y * (p.y - c.y)) / n.z)
            }
        }
    }
}

/// A drawn object: its region on the page, sampled as a polygon, and its
/// depth.
pub(super) struct Surface {
    pub region: Vec<V3>,
    pub depth: Depth,
    /// The depth that orders it, far first: its centre's.
    pub key: f64,
}

impl Surface {
    fn bbox(&self) -> (V3, V3) {
        self.region.iter().fold((V3::xy(f64::MAX, f64::MAX), V3::xy(f64::MIN, f64::MIN)), |(lo, hi), p| {
            (V3::xy(lo.x.min(p.x), lo.y.min(p.y)), V3::xy(hi.x.max(p.x), hi.y.max(p.y)))
        })
    }

    fn depth_at(&self, view: &View, p: V3) -> Option<f64> {
        if inside(&self.region, p) { self.depth.at(view, p) } else { None }
    }

    /// The depth at `p`, extended a stroke's width past the region's edge,
    /// where only the outline is drawn.
    fn stroke_depth_at(&self, view: &View, p: V3) -> Option<f64> {
        if inside(&self.region, p) { self.depth.at(view, p) } else { self.depth.within(view, p, MARGIN) }
    }
}

fn inside(poly: &[V3], p: V3) -> bool {
    let mut c = false;
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            c = !c;
        }
    }
    c
}

/// The drawing of a scene: each object's rank (drawn in increasing order)
/// and its holes, as sets of loops that do not overlap within a set, wound
/// clockwise.
pub(super) struct Composed {
    pub rank: Vec<u32>,
    pub holes: Vec<Vec<Vec<Path>>>,
}

/// A box on the page: its lower left and upper right corners.
type Bounds = (V3, V3);

/// How far a stroke may reach outside the region it outlines.
const MARGIN: f64 = 0.05;

/// Order `objects` far first and `planes` after them, far first, and find
/// every hole.
pub(super) fn compose(view: &View, objects: &[Surface], planes: &[Surface]) -> Composed {
    let n = objects.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| objects[a].key.total_cmp(&objects[b].key));
    let mut plane_order: Vec<usize> = (0..planes.len()).collect();
    plane_order.sort_by(|&a, &b| planes[a].key.total_cmp(&planes[b].key));
    let all: Vec<&Surface> = objects.iter().chain(planes.iter()).collect();
    let sequence: Vec<usize> = order.iter().copied().chain(plane_order.iter().map(|k| n + k)).collect();

    let mut rank = vec![0u32; all.len()];
    for (r, &k) in sequence.iter().enumerate() {
        rank[k] = r as u32;
    }
    let boxes: Vec<Bounds> = all.iter().map(|s| s.bbox()).collect();
    let mut holes: Vec<Vec<(Vec<Path>, Bounds)>> = vec![Vec::new(); all.len()];
    for (li, &later) in sequence.iter().enumerate() {
        for &earlier in &sequence[..li] {
            cut(view, (all[earlier], boxes[earlier]), (all[later], boxes[later]), &mut holes[later]);
        }
    }
    Composed { rank, holes: holes.into_iter().map(unbox).collect() }
}

/// The holes in `card`, drawn over everything, where any of `over` is
/// nearer.
pub(super) fn cover(view: &View, card: &Surface, over: &[&Surface]) -> Vec<Vec<Path>> {
    let mut holes = Vec::new();
    for e in over {
        cut(view, (e, e.bbox()), (card, card.bbox()), &mut holes);
    }
    unbox(holes)
}

fn unbox(holes: Vec<(Vec<Path>, Bounds)>) -> Vec<Vec<Path>> {
    holes.into_iter().map(|(set, _)| set).collect()
}

/// Add to the holes of `later` those where `earlier` is nearer.
fn cut(
    view: &View,
    (e, a): (&Surface, Bounds),
    (l, b): (&Surface, Bounds),
    holes: &mut Vec<(Vec<Path>, Bounds)>,
) {
    // The later's box grows by more than half a stroke, and so does its
    // depth, so that its outline, which lies half outside it, has holes too.
    let lo = V3::xy(a.0.x.max(b.0.x - MARGIN), a.0.y.max(b.0.y - MARGIN));
    let hi = V3::xy(a.1.x.min(b.1.x + MARGIN), a.1.y.min(b.1.y + MARGIN));
    if hi.x <= lo.x || hi.y <= lo.y {
        return;
    }
    // Where the earlier is there and nearer, or the later is not.
    let nearer = |p: V3| match e.depth_at(view, p) {
        Some(ze) => l.stroke_depth_at(view, p).is_none_or(|zl| ze > zl + 1e-9),
        None => false,
    };
    let loops = contour(lo, hi, &nearer);
    if loops.is_empty() {
        return;
    }
    let bounds = loops
        .iter()
        .flatten()
        .fold((V3::xy(f64::MAX, f64::MAX), V3::xy(f64::MIN, f64::MIN)), |(lo, hi), p| {
            (V3::xy(lo.x.min(p.x), lo.y.min(p.y)), V3::xy(hi.x.max(p.x), hi.y.max(p.y)))
        });
    let paths: Vec<Path> = loops.iter().map(|l| polygon(l)).collect();
    // Join sets whose loops cannot meet, so that holes nest less.
    match holes.iter_mut().find(|(_, bb)| {
        bb.1.x < bounds.0.x || bounds.1.x < bb.0.x || bb.1.y < bounds.0.y || bounds.1.y < bb.0.y
    }) {
        Some((set, bb)) => {
            set.extend(paths);
            *bb = (
                V3::xy(bb.0.x.min(bounds.0.x), bb.0.y.min(bounds.0.y)),
                V3::xy(bb.1.x.max(bounds.1.x), bb.1.y.max(bounds.1.y)),
            );
        }
        None => holes.push((paths, bounds)),
    }
}

fn polygon(pts: &[V3]) -> Path {
    let n = pts.len();
    Path {
        pieces: (0..n).map(|k| super::Piece::Line { a: pts[k], b: pts[(k + 1) % n] }).collect(),
        closed: true,
    }
}

/// The regions within the box `lo`..`hi` where `inside` holds, as loops
/// wound clockwise, by marching squares on a grid, with every crossing of a
/// grid edge refined by bisection.
fn contour(lo: V3, hi: V3, inside_at: &dyn Fn(V3) -> bool) -> Vec<Vec<V3>> {
    let size = (hi.x - lo.x).max(hi.y - lo.y);
    let h = (size / 40.0).max(0.004);
    let nx = ((hi.x - lo.x) / h).ceil() as usize + 1;
    let ny = ((hi.y - lo.y) / h).ceil() as usize + 1;
    let (dx, dy) = ((hi.x - lo.x) / (nx - 1).max(1) as f64, (hi.y - lo.y) / (ny - 1).max(1) as f64);
    // A frame of false points round the grid closes every loop.
    let at = |i: usize, j: usize| V3::xy(lo.x + (i as f64 - 1.0) * dx, lo.y + (j as f64 - 1.0) * dy);
    let within =
        |p: V3| p.x >= lo.x - 1e-12 && p.x <= hi.x + 1e-12 && p.y >= lo.y - 1e-12 && p.y <= hi.y + 1e-12;
    let f = |p: V3| within(p) && inside_at(p);
    let (w, hgt) = (nx + 2, ny + 2);
    let mut v = vec![false; w * hgt];
    let mut any = false;
    for j in 1..=ny {
        for i in 1..=nx {
            let t = f(at(i, j));
            v[j * w + i] = t;
            any |= t;
        }
    }
    if !any {
        return Vec::new();
    }
    let val = |i: usize, j: usize| v[j * w + i];
    // Edges: (horizontal?, i, j) from point (i, j) to (i+1, j) or (i, j+1).
    type Edge = (bool, usize, usize);
    let mut points: std::collections::BTreeMap<Edge, V3> = Default::default();
    let mut cross = |e: Edge| -> V3 {
        *points.entry(e).or_insert_with(|| {
            let (horiz, i, j) = e;
            let (p, q) = (at(i, j), if horiz { at(i + 1, j) } else { at(i, j + 1) });
            let (mut a, mut b) = (p, q);
            if !f(a) {
                std::mem::swap(&mut a, &mut b);
            }
            // a inside, b outside.
            for _ in 0..24 {
                let m = (a + b) * 0.5;
                if f(m) {
                    a = m;
                } else {
                    b = m;
                }
            }
            (a + b) * 0.5
        })
    };
    let mut next: std::collections::BTreeMap<Edge, Edge> = Default::default();
    for j in 0..hgt - 1 {
        for i in 0..w - 1 {
            // Corners counter-clockwise, and the edge leaving each.
            let c = [val(i, j), val(i + 1, j), val(i + 1, j + 1), val(i, j + 1)];
            let edges: [Edge; 4] = [(true, i, j), (false, i + 1, j), (true, i, j + 1), (false, i, j)];
            let outs: Vec<usize> = (0..4).filter(|&k| c[k] && !c[(k + 1) % 4]).collect();
            let ins: Vec<usize> = (0..4).filter(|&k| !c[k] && c[(k + 1) % 4]).collect();
            if outs.is_empty() {
                continue;
            }
            let pairs: Vec<(usize, usize)> = if outs.len() == 1 {
                vec![(outs[0], ins[0])]
            } else {
                // A saddle: the centre decides whether the true corners join.
                let centre = f((at(i, j) + at(i + 1, j + 1)) * 0.5);
                outs.iter()
                    .map(|&o| {
                        let step = if centre { 1 } else { 3 };
                        let k = (o + step) % 4;
                        (o, if ins.contains(&k) { k } else { (o + 4 - step) % 4 })
                    })
                    .collect()
            };
            for (o, n_in) in pairs {
                next.insert(edges[o], edges[n_in]);
            }
        }
    }
    // Follow the segments into loops, true on their left; then turn them
    // clockwise.
    let mut loops = Vec::new();
    while let Some(&start) = next.keys().next() {
        let mut lp = Vec::new();
        let mut e = start;
        while let Some(n) = next.remove(&e) {
            lp.push(cross(e));
            e = n;
            if e == start {
                break;
            }
        }
        if lp.len() >= 3 {
            lp.reverse();
            loops.push(lp);
        }
    }
    loops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(lp: &[V3]) -> f64 {
        (0..lp.len())
            .map(|k| {
                let (a, b) = (lp[k], lp[(k + 1) % lp.len()]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f64>()
            / 2.0
    }

    #[test]
    fn contours_follow_curves_clockwise() {
        // A disc of radius .5.
        let loops = contour(V3::xy(-1.0, -1.0), V3::xy(1.0, 1.0), &|p| p.norm() < 0.5);
        assert_eq!(loops.len(), 1);
        let a = area(&loops[0]);
        assert!(a < 0.0, "clockwise");
        assert!((a.abs() - std::f64::consts::PI * 0.25).abs() < 0.01, "{a}");
        for p in &loops[0] {
            assert!((p.norm() - 0.5).abs() < 1e-4);
        }
        // An annulus: two loops, the inner one counter-clockwise.
        let loops = contour(V3::xy(-1.0, -1.0), V3::xy(1.0, 1.0), &|p| p.norm() < 0.8 && p.norm() > 0.4);
        assert_eq!(loops.len(), 2);
        let areas: Vec<f64> = loops.iter().map(|l| area(l)).collect();
        assert!(areas.iter().any(|&a| a < 0.0) && areas.iter().any(|&a| a > 0.0));
        // Cut by the box.
        let loops = contour(V3::xy(0.0, 0.0), V3::xy(1.0, 1.0), &|p| p.norm() < 0.5);
        assert!((area(&loops[0]).abs() - std::f64::consts::PI * 0.25 / 4.0).abs() < 0.005);
    }

    #[test]
    fn tubes_are_deepest_on_their_axis() {
        let view = View::camera(0.0, 90.0);
        let d = Depth::Tube { pts: vec![V3::new(-1.0, 0.0, 0.0), V3::new(1.0, 0.0, 0.0)], r: 0.2 };
        let on = d.at(&view, V3::xy(0.0, 0.0)).unwrap();
        let off = d.at(&view, V3::xy(0.0, 0.15)).unwrap();
        assert!((on - 0.2).abs() < 1e-9 && off < on);
        assert!(d.at(&view, V3::xy(0.0, 0.3)).is_none());
    }

    #[test]
    fn slanted_tubes_have_their_true_front() {
        // A tube leaning towards the viewer, against its surface sampled
        // along each line of sight.
        let view = View::camera(0.0, 90.0);
        let (a, b, r) = (V3::new(0.0, -1.0, -1.0), V3::new(0.0, 1.0, 1.5), 0.3);
        let d = Depth::Tube { pts: vec![a, b], r };
        let axis = (b - a).unit().unwrap();
        for p in [V3::xy(0.0, 0.0), V3::xy(0.2, 0.3), V3::xy(-0.25, -0.4)] {
            let mut z = -10.0f64;
            let mut zs = 3.0;
            while zs > -3.0 {
                let x = V3::new(p.x, p.y, zs);
                let u = x - a;
                let t = (u.x * axis.x + u.y * axis.y + u.z * axis.z).clamp(0.0, (b - a).norm());
                if (x - (a + axis * t)).norm() <= r {
                    z = zs;
                    break;
                }
                zs -= 1e-5;
            }
            let got = d.at(&view, p).unwrap();
            assert!((got - z).abs() < 1e-3, "{p:?}: {got} against {z}");
        }
    }
}
