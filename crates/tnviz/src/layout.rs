//! The layout engine: positions of tensors, directions of open legs, and
//! routes of bonds, in layout units.
//!
//! Layout statements fix relative positions: `chain`, `grid`, `tree`,
//! `stack`, and relative placement join tensors into rigid blocks, and `at`
//! pins a block in place.  Tensors that no statement places are laid out
//! automatically by stress majorization over the bond graph, in which rigid
//! blocks only translate and pinned blocks stay put.  Everything is
//! deterministic.
//!
//! The result says nothing about shapes: where a bond meets a tensor's
//! outline is decided later, when shapes are known.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ops::{Add, Mul, Sub};

use crate::error::{Error, Result};
use crate::model::{Direction, IndexId, Layout as Stmt, Network, Relation, TensorId};
use crate::value::{Attr, Value};

// ---- Vectors --------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub const ZERO: V3 = V3 { x: 0.0, y: 0.0, z: 0.0 };

    pub fn new(x: f64, y: f64, z: f64) -> Self {
        V3 { x, y, z }
    }

    pub fn xy(x: f64, y: f64) -> Self {
        V3 { x, y, z: 0.0 }
    }

    pub fn norm(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn unit(self) -> Option<V3> {
        let n = self.norm();
        (n > 1e-12).then(|| self * (1.0 / n))
    }

    /// Rotate about the z axis by `deg` degrees.
    pub fn rotate_z(self, deg: f64) -> V3 {
        let (s, c) = deg.to_radians().sin_cos();
        V3 { x: c * self.x - s * self.y, y: s * self.x + c * self.y, z: self.z }
    }

    fn from_slice(v: &[f64]) -> V3 {
        V3 { x: v[0], y: v[1], z: v.get(2).copied().unwrap_or(0.0) }
    }

    fn close(self, other: V3) -> bool {
        (self - other).norm() < 1e-9
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        V3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, k: f64) -> V3 {
        V3::new(self.x * k, self.y * k, self.z * k)
    }
}

// ---- Result ---------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct LayoutOptions {
    /// Distance between neighbouring tensors in chains, grids, trees,
    /// stacks, and relative placements without a distance.
    pub spacing: f64,
    /// Length of open legs without `leg-length`.
    pub leg_length: f64,
    /// Sideways offset between parallel bonds.
    pub parallel_offset: f64,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        LayoutOptions { spacing: 2.0, leg_length: 0.6, parallel_offset: 0.35 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedTensor {
    pub pos: V3,
    /// Rotation about the z axis, in degrees.
    pub rotation: f64,
}

/// One end of a bond.
#[derive(Clone, Debug, PartialEq)]
pub struct BondEnd {
    pub tensor: TensorId,
    pub slot: usize,
    /// The direction in which the bond leaves the tensor, when a port is
    /// given; otherwise the bond heads straight for its route.
    pub dir: Option<V3>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedBond {
    pub index: IndexId,
    pub a: BondEnd,
    pub b: BondEnd,
    /// Intermediate points of the centreline, from `a` to `b`.
    pub via: Vec<V3>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedLeg {
    pub tensor: TensorId,
    pub slot: usize,
    pub index: IndexId,
    /// Unit direction in world coordinates.
    pub dir: V3,
    pub length: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Indexed by `TensorId`.
    pub tensors: Vec<PlacedTensor>,
    pub bonds: Vec<PlacedBond>,
    pub legs: Vec<PlacedLeg>,
}

impl Layout {
    pub fn pos(&self, t: TensorId) -> V3 {
        self.tensors[t.0].pos
    }
}

/// Lay out a network.
pub fn layout(net: &Network, opts: &LayoutOptions) -> Result<Layout> {
    let n = net.tensors().len();
    let mut rigid = Rigid::new(n);
    for stmt in &net.layout {
        apply(net, opts, &mut rigid, stmt)?;
    }
    let pos = place(net, opts, &mut rigid);
    let tensors = (0..n)
        .map(|k| PlacedTensor { pos: pos[k], rotation: number(&net.tensor_style(TensorId(k)), "rotate") })
        .collect();
    let mut out = Layout { tensors, bonds: Vec::new(), legs: Vec::new() };
    route(net, opts, &mut out);
    Ok(out)
}

// ---- Rigid blocks ---------------------------------------------------------

/// Union-find over tensors that keeps each tensor's offset from its root,
/// and an optional absolute position per root.
struct Rigid {
    parent: Vec<usize>,
    offset: Vec<V3>,
    pinned: HashMap<usize, V3>,
}

impl Rigid {
    fn new(n: usize) -> Self {
        Rigid { parent: (0..n).collect(), offset: vec![V3::ZERO; n], pinned: HashMap::new() }
    }

    /// The root of `i` and the offset of `i` from it.
    fn find(&mut self, i: usize) -> (usize, V3) {
        if self.parent[i] == i {
            return (i, V3::ZERO);
        }
        let (root, parent_off) = self.find(self.parent[i]);
        self.parent[i] = root;
        self.offset[i] = self.offset[i] + parent_off;
        (root, self.offset[i])
    }

    /// Require pos(j) - pos(i) = d.
    fn relate(&mut self, i: usize, j: usize, d: V3) -> std::result::Result<(), ()> {
        let (ri, oi) = self.find(i);
        let (rj, oj) = self.find(j);
        if ri == rj {
            return if (oj - oi).close(d) { Ok(()) } else { Err(()) };
        }
        // Hang rj under ri: pos(rj) = pos(ri) + oi + d - oj.
        let off = oi + d - oj;
        match (self.pinned.get(&ri).copied(), self.pinned.remove(&rj)) {
            (Some(pi), Some(pj)) if !(pi + off).close(pj) => {
                self.pinned.insert(rj, pj);
                return Err(());
            }
            (None, Some(pj)) => {
                self.pinned.insert(ri, pj - off);
            }
            _ => {}
        }
        self.parent[rj] = ri;
        self.offset[rj] = off;
        Ok(())
    }

    fn pin(&mut self, i: usize, p: V3) -> std::result::Result<(), ()> {
        let (r, o) = self.find(i);
        let root = p - o;
        match self.pinned.get(&r) {
            Some(existing) if !existing.close(root) => Err(()),
            _ => {
                self.pinned.insert(r, root);
                Ok(())
            }
        }
    }
}

fn apply(net: &Network, opts: &LayoutOptions, rigid: &mut Rigid, stmt: &Stmt) -> Result<()> {
    let s = opts.spacing;
    let name = |t: TensorId| net.tensor(t).name.to_string();
    let conflict = |what: &str| Error::new(format!("layout conflict: {what} contradicts earlier placement"));
    match stmt {
        Stmt::At { tensor, pos } => rigid
            .pin(tensor.0, V3::from_slice(pos))
            .map_err(|_| conflict(&format!("{} at …", name(*tensor))))?,
        Stmt::Relative { tensor, relation, anchor, distance } => {
            let d = distance.unwrap_or(s);
            let v = match relation {
                Relation::RightOf => V3::xy(d, 0.0),
                Relation::LeftOf => V3::xy(-d, 0.0),
                Relation::Above => V3::xy(0.0, d),
                Relation::Below => V3::xy(0.0, -d),
            };
            rigid.relate(anchor.0, tensor.0, v).map_err(|_| {
                conflict(&format!("the position of {} relative to {}", name(*tensor), name(*anchor)))
            })?
        }
        Stmt::Row { tensors } => {
            for (k, t) in tensors.iter().enumerate().skip(1) {
                rigid
                    .relate(tensors[0].0, t.0, V3::xy(k as f64 * s, 0.0))
                    .map_err(|_| conflict(&format!("the chain through {}", name(*t))))?;
            }
        }
        Stmt::Grid { base, rows, cols } => {
            let id = |r: usize, c: usize| {
                net.find_tensor(&crate::name::Name::new(base.clone(), [r as i64, c as i64])).unwrap()
            };
            let origin = id(1, 1);
            for r in 1..=*rows {
                for c in 1..=*cols {
                    let d = V3::xy((c - 1) as f64 * s, -((r - 1) as f64) * s);
                    rigid
                        .relate(origin.0, id(r, c).0, d)
                        .map_err(|_| conflict(&format!("the grid {base}")))?;
                }
            }
        }
        Stmt::Stack { groups } => {
            let firsts: Vec<TensorId> = groups
                .iter()
                .map(|g| {
                    net.group(g)
                        .and_then(|g| g.members.first().copied())
                        .ok_or_else(|| Error::new(format!("group `{g}` is empty")))
                })
                .collect::<Result<_>>()?;
            for w in firsts.windows(2) {
                rigid.relate(w[0].0, w[1].0, V3::xy(0.0, -s)).map_err(|_| conflict("the stack"))?;
            }
        }
        Stmt::Tree { root, direction } => {
            for (t, d) in tree(net, *root, s) {
                rigid.relate(root.0, t.0, orient(d, direction)?).map_err(|_| conflict("the tree"))?;
            }
        }
    }
    Ok(())
}

/// Tree positions relative to the root, growing downwards: leaves sit
/// `spacing` apart and each parent is centred over its children.
fn tree(net: &Network, root: TensorId, s: f64) -> Vec<(TensorId, V3)> {
    let adj = adjacency(net);
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut seen = vec![false; adj.len()];
    let mut depth = vec![0usize; adj.len()];
    let mut queue = VecDeque::from([root.0]);
    seen[root.0] = true;
    while let Some(u) = queue.pop_front() {
        for &v in &adj[u] {
            if !seen[v] {
                seen[v] = true;
                depth[v] = depth[u] + 1;
                children.entry(u).or_default().push(v);
                queue.push_back(v);
            }
        }
    }
    let mut x = vec![0.0; adj.len()];
    let mut next_leaf = 0.0;
    fn assign(u: usize, ch: &HashMap<usize, Vec<usize>>, x: &mut [f64], next: &mut f64, s: f64) {
        match ch.get(&u) {
            None => {
                x[u] = *next;
                *next += s;
            }
            Some(kids) => {
                for &k in kids {
                    assign(k, ch, x, next, s);
                }
                x[u] = (x[kids[0]] + x[kids[kids.len() - 1]]) / 2.0;
            }
        }
    }
    assign(root.0, &children, &mut x, &mut next_leaf, s);
    (0..adj.len())
        .filter(|&t| seen[t])
        .map(|t| (TensorId(t), V3::xy(x[t] - x[root.0], -(depth[t] as f64) * s)))
        .collect()
}

/// Turn a downward-growing offset to face `dir`.
fn orient(d: V3, dir: &Direction) -> Result<V3> {
    let down = V3::xy(0.0, -1.0);
    let target = direction_vector(dir).ok_or_else(|| Error::new("a tree grows in a 2D direction"))?;
    let angle = target.y.atan2(target.x).to_degrees() - down.y.atan2(down.x).to_degrees();
    Ok(d.rotate_z(angle))
}

/// Neighbours along bonds, sorted and without repeats.
fn adjacency(net: &Network) -> Vec<Vec<usize>> {
    let mut adj = vec![Vec::new(); net.tensors().len()];
    for (_, index) in net.bonds() {
        let (a, b) = (index.holders()[0].0.0, index.holders()[1].0.0);
        if a != b {
            adj[a].push(b);
            adj[b].push(a);
        }
    }
    for list in &mut adj {
        list.sort_unstable();
        list.dedup();
    }
    adj
}

// ---- Automatic placement ----------------------------------------------------

fn place(net: &Network, opts: &LayoutOptions, rigid: &mut Rigid) -> Vec<V3> {
    let n = net.tensors().len();
    let s = opts.spacing;
    let adj = adjacency(net);
    let roots: Vec<usize> = (0..n).map(|i| rigid.find(i).0).collect();
    let offsets: Vec<V3> = (0..n).map(|i| rigid.find(i).1).collect();
    let mut blocks: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, r) in roots.iter().enumerate() {
        blocks.entry(*r).or_default().push(i);
    }

    // Connected parts: bonds and rigid blocks both connect.
    let mut part = vec![usize::MAX; n];
    let mut parts: Vec<Vec<usize>> = Vec::new();
    for start in 0..n {
        if part[start] != usize::MAX {
            continue;
        }
        let id = parts.len();
        let mut members = Vec::new();
        let mut stack = vec![start];
        part[start] = id;
        while let Some(u) = stack.pop() {
            members.push(u);
            for &v in adj[u].iter().chain(&blocks[&roots[u]]) {
                if part[v] == usize::MAX {
                    part[v] = id;
                    stack.push(v);
                }
            }
        }
        members.sort_unstable();
        parts.push(members);
    }

    let mut pos = vec![V3::ZERO; n];
    let mut packed: Vec<(usize, bool)> = Vec::new();
    for (k, members) in parts.iter().enumerate() {
        let pinned_part = members.iter().any(|&i| rigid.pinned.contains_key(&roots[i]));
        place_part(members, &adj, &roots, &offsets, &rigid.pinned, &blocks, s, &mut pos);
        packed.push((k, pinned_part));
    }

    // Parts with nothing pinned go left to right, after the pinned ones.
    let mut cursor = parts
        .iter()
        .zip(&packed)
        .filter(|(_, (_, pinned))| *pinned)
        .flat_map(|(m, _)| m.iter().map(|&i| pos[i].x))
        .fold(f64::NEG_INFINITY, f64::max);
    let mut first = cursor == f64::NEG_INFINITY;
    for (members, (_, pinned)) in parts.iter().zip(&packed) {
        if *pinned {
            continue;
        }
        let min_x = members.iter().map(|&i| pos[i].x).fold(f64::INFINITY, f64::min);
        let max_x = members.iter().map(|&i| pos[i].x).fold(f64::NEG_INFINITY, f64::max);
        let shift = if first { -min_x } else { cursor + s - min_x };
        for &i in members {
            pos[i].x += shift;
        }
        cursor = max_x + shift;
        first = false;
    }
    pos
}

/// Place one connected part.
///
/// With pinned blocks, free blocks start next to an already placed
/// neighbour.  Without, the part starts from classical multidimensional
/// scaling of hop distances, which gives the global shape and avoids folds.
/// Stress majorization then refines the positions, moving rigid blocks
/// only by translation and pinned blocks not at all.  A part of single
/// tensors is finally turned to a canonical orientation.
#[allow(clippy::too_many_arguments)]
fn place_part(
    members: &[usize],
    adj: &[Vec<usize>],
    roots: &[usize],
    offsets: &[V3],
    pinned: &HashMap<usize, V3>,
    blocks: &BTreeMap<usize, Vec<usize>>,
    s: f64,
    pos: &mut [V3],
) {
    let mut part_roots: Vec<usize> = members.iter().map(|&i| roots[i]).collect();
    part_roots.sort_unstable();
    part_roots.dedup();
    let fixed: HashMap<usize, V3> =
        part_roots.iter().filter_map(|r| pinned.get(r).map(|p| (*r, *p))).collect();
    let dist = hop_distances(members, adj);
    let at = |i: usize, root_pos: &HashMap<usize, V3>| root_pos.get(&roots[i]).map(|p| *p + offsets[i]);

    let mut root_pos: HashMap<usize, V3>;
    if fixed.is_empty() {
        let init = classical_mds(&dist, s);
        let mut sum: HashMap<usize, (V3, f64)> = HashMap::new();
        for (k, &i) in members.iter().enumerate() {
            let e = sum.entry(roots[i]).or_insert((V3::ZERO, 0.0));
            e.0 = e.0 + (init[k] - offsets[i]);
            e.1 += 1.0;
        }
        root_pos = sum.into_iter().map(|(r, (p, c))| (r, p * (1.0 / c))).collect();
    } else {
        root_pos = fixed.clone();
        let mut start: Vec<usize> = fixed.keys().copied().collect();
        start.sort_unstable();
        let mut queue = VecDeque::from(start);
        while let Some(r) = queue.pop_front() {
            for &u in &blocks[&r] {
                for &v in &adj[u] {
                    let rv = roots[v];
                    if root_pos.contains_key(&rv) {
                        continue;
                    }
                    let base = at(u, &root_pos).unwrap();
                    let occupied: Vec<V3> = members.iter().filter_map(|&w| at(w, &root_pos)).collect();
                    root_pos.insert(rv, free_spot(base, &occupied, s) - offsets[v]);
                    queue.push_back(rv);
                }
            }
        }
        for r in &part_roots {
            root_pos.entry(*r).or_insert(V3::ZERO);
        }
    }

    stress_majorization(members, &dist, roots, offsets, &fixed, s, &mut root_pos);

    if fixed.is_empty() && part_roots.iter().all(|r| blocks[r].len() == 1) {
        canonical_orientation(members, adj, &mut root_pos);
    }
    for &i in members {
        pos[i] = at(i, &root_pos).unwrap();
    }
}

/// Stress majorization: pull every pair towards `hops * s` apart, weighted
/// by the inverse square distance.  Each rigid block moves by the mean of
/// its members' updates; fixed blocks do not move.
fn stress_majorization(
    members: &[usize],
    dist: &[Vec<Option<usize>>],
    roots: &[usize],
    offsets: &[V3],
    fixed: &HashMap<usize, V3>,
    s: f64,
    root_pos: &mut HashMap<usize, V3>,
) {
    for _ in 0..500 {
        let current: Vec<V3> = members.iter().map(|&i| root_pos[&roots[i]] + offsets[i]).collect();
        let mut shift: BTreeMap<usize, (V3, f64)> = BTreeMap::new();
        for (a, &i) in members.iter().enumerate() {
            if fixed.contains_key(&roots[i]) {
                continue;
            }
            let (mut num, mut den) = (V3::ZERO, 0.0);
            for (b, &j) in members.iter().enumerate() {
                let Some(h) = dist[a][b] else { continue };
                if roots[i] == roots[j] {
                    continue;
                }
                let d = h as f64 * s;
                let w = 1.0 / (d * d);
                let diff = current[a] - current[b];
                let len = diff.norm().max(1e-9);
                num = num + (current[b] + diff * (d / len)) * w;
                den += w;
            }
            if den > 0.0 {
                let e = shift.entry(roots[i]).or_insert((V3::ZERO, 0.0));
                e.0 = e.0 + (num * (1.0 / den) - current[a]);
                e.1 += 1.0;
            }
        }
        let mut moved = 0.0f64;
        for (r, (sum, count)) in shift {
            let step = sum * (1.0 / count);
            moved = moved.max(step.norm());
            let p = root_pos[&r] + step;
            root_pos.insert(r, p);
        }
        if moved < 1e-7 * s {
            break;
        }
    }
}

/// Classical multidimensional scaling of hop distances into the plane,
/// by power iteration from fixed starting vectors.
fn classical_mds(dist: &[Vec<Option<usize>>], s: f64) -> Vec<V3> {
    let m = dist.len();
    if m == 1 {
        return vec![V3::ZERO];
    }
    let far = dist.iter().flatten().flatten().max().copied().unwrap_or(1) + 1;
    let d2: Vec<Vec<f64>> =
        dist.iter().map(|row| row.iter().map(|h| (h.unwrap_or(far) as f64 * s).powi(2)).collect()).collect();
    let mean: Vec<f64> = d2.iter().map(|row| row.iter().sum::<f64>() / m as f64).collect();
    let grand = mean.iter().sum::<f64>() / m as f64;
    let b: Vec<Vec<f64>> =
        (0..m).map(|i| (0..m).map(|j| -0.5 * (d2[i][j] - mean[i] - mean[j] + grand)).collect()).collect();
    let apply =
        |x: &[f64]| -> Vec<f64> { b.iter().map(|row| row.iter().zip(x).map(|(a, c)| a * c).sum()).collect() };
    let normalize = |x: &mut Vec<f64>| {
        let n = x.iter().map(|v| v * v).sum::<f64>().sqrt();
        if n > 1e-12 {
            x.iter_mut().for_each(|v| *v /= n);
        }
    };
    let dot = |x: &[f64], y: &[f64]| x.iter().zip(y).map(|(a, c)| a * c).sum::<f64>();
    let mut vectors: Vec<(Vec<f64>, f64)> = Vec::new();
    for seed in [0.7f64, 1.3] {
        let mut x: Vec<f64> = (0..m).map(|i| (seed * (i as f64 + 1.0)).cos() + 0.5 * seed).collect();
        for _ in 0..500 {
            for (v, _) in &vectors {
                let p = dot(&x, v);
                x.iter_mut().zip(v).for_each(|(a, c)| *a -= p * c);
            }
            normalize(&mut x);
            let mut y = apply(&x);
            for (v, _) in &vectors {
                let p = dot(&y, v);
                y.iter_mut().zip(v).for_each(|(a, c)| *a -= p * c);
            }
            normalize(&mut y);
            x = y;
        }
        let lambda = dot(&x, &apply(&x)).max(0.0);
        vectors.push((x, lambda));
    }
    (0..m)
        .map(|i| {
            let (v1, l1) = &vectors[0];
            let (v2, l2) = &vectors[1];
            // A small bend keeps collinear starts from staying collinear.
            V3::xy(v1[i] * l1.sqrt(), v2[i] * l2.sqrt() + 1e-3 * s * (i as f64).sin())
        })
        .collect()
}

/// Turn a part of single tensors to a canonical orientation.  The part is
/// first rotated so that its bonds lie as close as possible to horizontal
/// and vertical (the circular mean of four times the bond angles).  Among
/// the four right-angle turns of that, the one that points the bond from the
/// first tensor to its first neighbour closest to the right is kept, and the
/// part is mirrored so that the second neighbour lies below.  A lattice then
/// reads left to right and top to bottom.
fn canonical_orientation(members: &[usize], adj: &[Vec<usize>], root_pos: &mut HashMap<usize, V3>) {
    let first = members[0];
    let neighbours: Vec<usize> = adj[first].iter().copied().filter(|v| members.contains(v)).collect();
    let Some(&n1) = neighbours.first() else { return };
    let (mut sin4, mut cos4) = (0.0, 0.0);
    for &u in members {
        for &v in &adj[u] {
            if u < v {
                let d = root_pos[&v] - root_pos[&u];
                let t = 4.0 * d.y.atan2(d.x);
                sin4 += t.sin();
                cos4 += t.cos();
            }
        }
    }
    let mut angle = -f64::atan2(sin4, cos4).to_degrees() / 4.0;
    let d = (root_pos[&n1] - root_pos[&first]).rotate_z(angle);
    angle -= (d.y.atan2(d.x).to_degrees() / 90.0).round() * 90.0;
    let origin = root_pos[&first];
    for p in root_pos.values_mut() {
        *p = origin + (*p - origin).rotate_z(angle);
    }
    if let Some(&n2) = neighbours.get(1)
        && root_pos[&n2].y > origin.y
    {
        for p in root_pos.values_mut() {
            p.y = 2.0 * origin.y - p.y;
        }
    }
}

/// A spot one spacing away from `base`, trying directions in a fixed order
/// and taking the first that keeps clear of occupied points.
fn free_spot(base: V3, occupied: &[V3], s: f64) -> V3 {
    let angles = [0.0, -90.0, 90.0, 180.0, -45.0, 45.0, -135.0, 135.0];
    for ring in 1..=4 {
        for a in angles {
            let p = base + V3::xy(ring as f64 * s, 0.0).rotate_z(a);
            if occupied.iter().all(|q| (*q - p).norm() > 0.5 * s) {
                return p;
            }
        }
    }
    base + V3::xy(5.0 * s, 0.0)
}

/// Hop distances between members of a part, `None` when not connected by
/// bonds.
fn hop_distances(members: &[usize], adj: &[Vec<usize>]) -> Vec<Vec<Option<usize>>> {
    let index_of: HashMap<usize, usize> = members.iter().enumerate().map(|(k, &i)| (i, k)).collect();
    members
        .iter()
        .map(|&start| {
            let mut d = vec![None; members.len()];
            d[index_of[&start]] = Some(0);
            let mut queue = VecDeque::from([start]);
            while let Some(u) = queue.pop_front() {
                let du = d[index_of[&u]].unwrap();
                for &v in &adj[u] {
                    let k = index_of[&v];
                    if d[k].is_none() {
                        d[k] = Some(du + 1);
                        queue.push_back(v);
                    }
                }
            }
            d
        })
        .collect()
}

// ---- Legs and bonds ---------------------------------------------------------

/// A unit vector for a direction, in the frame it is given in.
pub fn direction_vector(dir: &Direction) -> Option<V3> {
    let d = match dir {
        Direction::Word(w) => match w.as_str() {
            "up" => V3::xy(0.0, 1.0),
            "down" => V3::xy(0.0, -1.0),
            "left" => V3::xy(-1.0, 0.0),
            "right" => V3::xy(1.0, 0.0),
            "up-left" => V3::xy(-1.0, 1.0),
            "up-right" => V3::xy(1.0, 1.0),
            "down-left" => V3::xy(-1.0, -1.0),
            "down-right" => V3::xy(1.0, -1.0),
            _ => return None,
        },
        Direction::Angle(a) => V3::xy(a.to_radians().cos(), a.to_radians().sin()),
        Direction::Axis(a) => {
            let sign = if a.starts_with('-') { -1.0 } else { 1.0 };
            match &a[1..] {
                "x" => V3::new(sign, 0.0, 0.0),
                "y" => V3::new(0.0, sign, 0.0),
                "z" => V3::new(0.0, 0.0, sign),
                _ => return None,
            }
        }
        Direction::Vector(v) => V3::from_slice(v),
    };
    d.unit()
}

/// A direction stored as an attribute value.
fn value_direction(v: &Value) -> Option<Direction> {
    Some(match v {
        Value::Number(a) => Direction::Angle(*a),
        Value::Word(w) if w.starts_with('+') || w.starts_with('-') => Direction::Axis(w.clone()),
        Value::Word(w) => Direction::Word(w.clone()),
        Value::Points(p) if p.len() == 1 => Direction::Vector(p[0].clone()),
        _ => return None,
    })
}

fn number(attrs: &[Attr], key: &str) -> f64 {
    match attrs.iter().find(|a| a.key == key).map(|a| &a.value) {
        Some(Value::Number(x)) => *x,
        _ => 0.0,
    }
}

/// The world direction of a slot's `leg-dir`, if set.
fn slot_direction(net: &Network, out: &Layout, t: TensorId, slot: usize) -> Option<V3> {
    let style = net.leg_style(t, slot);
    let v = style.iter().find(|a| a.key == "leg-dir")?;
    let local = direction_vector(&value_direction(&v.value)?)?;
    Some(local.rotate_z(out.tensors[t.0].rotation))
}

fn route(net: &Network, opts: &LayoutOptions, out: &mut Layout) {
    // Open legs.
    for (id, index) in net.open_legs() {
        let (t, slot) = index.holders()[0];
        let dir = slot_direction(net, out, t, slot).unwrap_or_else(|| {
            let local = if index.prime == 0 { V3::xy(0.0, -1.0) } else { V3::xy(0.0, 1.0) };
            local.rotate_z(out.tensors[t.0].rotation)
        });
        let style = net.leg_style(t, slot);
        let length = match style.iter().find(|a| a.key == "leg-length").map(|a| &a.value) {
            Some(Value::Number(x)) => *x,
            _ => opts.leg_length,
        };
        out.legs.push(PlacedLeg { tensor: t, slot, index: id, dir, length });
    }

    // Bonds, with parallel bonds between the same pair fanned out.
    let mut by_pair: BTreeMap<(usize, usize), Vec<IndexId>> = BTreeMap::new();
    for (id, index) in net.bonds() {
        let (a, b) = (index.holders()[0].0.0, index.holders()[1].0.0);
        by_pair.entry((a.min(b), a.max(b))).or_default().push(id);
    }
    for ((ta, tb), ids) in by_pair {
        let count = ids.len();
        for (k, id) in ids.into_iter().enumerate() {
            let index = net.index(id);
            let [(a, sa), (b, sb)] = [index.holders()[0], index.holders()[1]];
            let end = |t: TensorId, slot: usize| BondEnd {
                tensor: t,
                slot,
                dir: slot_direction(net, out, t, slot),
            };
            let style = net.bond_style(id);
            let mut via: Vec<V3> = match style.iter().find(|a| a.key == "via").map(|a| &a.value) {
                Some(Value::Points(points)) => points.iter().map(|p| V3::from_slice(p)).collect(),
                _ => Vec::new(),
            };
            if via.is_empty() {
                let (pa, pb) = (out.pos(a), out.pos(b));
                if ta == tb {
                    // A loop over the tensor.
                    let r = opts.spacing * 0.35 * (k + 1) as f64;
                    via = vec![pa + V3::xy(-r * 0.6, r * 1.4), pa + V3::xy(r * 0.6, r * 1.4)];
                } else if count > 1 {
                    let side = k as f64 - (count - 1) as f64 / 2.0;
                    let normal = V3::xy(-(pb - pa).y, (pb - pa).x).unit().unwrap_or(V3::xy(0.0, 1.0));
                    via = vec![(pa + pb) * 0.5 + normal * (side * opts.parallel_offset)];
                }
            }
            out.bonds.push(PlacedBond { index: id, a: end(a, sa), b: end(b, sb), via });
        }
    }
}
