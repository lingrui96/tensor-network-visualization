//! The layout engine.

use tnviz::layout::V3;
use tnviz::{Layout, LayoutOptions, Name, Network, layout, parse};

fn lay(src: &str) -> (Network, Layout) {
    let net = parse(src).unwrap_or_else(|e| panic!("{e}"));
    let lay = layout(&net, &LayoutOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    (net, lay)
}

fn pos(net: &Network, lay: &Layout, name: &str, subs: &[i64]) -> V3 {
    lay.pos(net.find_tensor(&Name::new(name, subs.to_vec())).unwrap())
}

fn close(a: V3, b: V3) -> bool {
    (a - b).norm() < 1e-6
}

#[test]
fn chain_grid_and_stack() {
    let (n, l) = lay("ket: chain A[1..3]\nop: chain W[1..3]\nA[1..3] - W[1..3]\nstack op, ket\n");
    assert!(close(pos(&n, &l, "A", &[3]) - pos(&n, &l, "A", &[1]), V3::xy(4.0, 0.0)));
    assert!(close(pos(&n, &l, "A", &[2]) - pos(&n, &l, "W", &[2]), V3::xy(0.0, -2.0)));

    let (n, l) = lay("grid T 2x3\n");
    assert!(close(pos(&n, &l, "T", &[2, 3]) - pos(&n, &l, "T", &[1, 1]), V3::xy(4.0, -2.0)));
}

#[test]
fn tree_centres_parents() {
    let (n, l) = lay("R - L[1], L[2]\nL[1] - X[1], X[2]\nL[2] - X[3]\ntree R down\n");
    let x = |name: &str, s: &[i64]| pos(&n, &l, name, s);
    assert!((x("L", &[1]).x - (x("X", &[1]).x + x("X", &[2]).x) / 2.0).abs() < 1e-9);
    assert!((x("R", &[]).x - (x("L", &[1]).x + x("L", &[2]).x) / 2.0).abs() < 1e-9);
    assert!(x("X", &[1]).y < x("L", &[1]).y && x("L", &[1]).y < x("R", &[]).y);

    let (n, l) = lay("R - C[1], C[2]\ntree R right\n");
    assert!(pos(&n, &l, "C", &[1]).x > pos(&n, &l, "R", &[]).x);
}

#[test]
fn pins_and_relative_placement() {
    let (n, l) = lay("chain A[1..3]\nA[2] at (10, 5)\nB below A[3], 3\n");
    assert!(close(pos(&n, &l, "A", &[1]), V3::xy(8.0, 5.0)));
    assert!(close(pos(&n, &l, "B", &[]), V3::xy(12.0, 2.0)));
}

#[test]
fn conflicts_are_errors() {
    let net = parse("chain A[1..2]\nA[1] at (0, 0)\nA[2] at (5, 0)\n").unwrap();
    let err = layout(&net, &LayoutOptions::default()).unwrap_err().to_string();
    assert!(err.contains("layout conflict"), "{err}");
    let net = parse("chain A, B\nB right of A, 3\n").unwrap();
    assert!(layout(&net, &LayoutOptions::default()).is_err());
}

#[test]
fn automatic_layout_is_deterministic_and_spread() {
    let src = "H - R[1], R[3], R[5]\nR[1] - R[2] - R[3] - R[4] - R[5] - R[6] - R[1]\nX - Y\n";
    let (n, l1) = lay(src);
    let (_, l2) = lay(src);
    assert_eq!(l1, l2);
    let points: Vec<V3> = l1.tensors.iter().map(|t| t.pos).collect();
    for (i, a) in points.iter().enumerate() {
        for b in &points[i + 1..] {
            assert!((*a - *b).norm() > 1.0, "tensors overlap");
        }
    }
    // Bonded tensors sit near the spacing apart.
    let h = pos(&n, &l1, "H", &[]);
    let r1 = pos(&n, &l1, "R", &[1]);
    assert!(((h - r1).norm() - 2.0).abs() < 0.6);
    // The separate pair is placed to the right of the ring.
    let ring_right = points.iter().take(7).map(|p| p.x).fold(f64::MIN, f64::max);
    assert!(pos(&n, &l1, "X", &[]).x > ring_right);
}

#[test]
fn automatic_placement_around_pinned_tensors() {
    let (n, l) = lay("A at (0, 0)\nA - B\n");
    assert!(close(pos(&n, &l, "A", &[]), V3::ZERO));
    assert!(((pos(&n, &l, "B", &[]) - V3::ZERO).norm() - 2.0).abs() < 1e-6);
}

#[test]
fn legs_follow_prime_level_and_rotation() {
    let (n, l) = lay("tensor W (s, s', a)\nA: leg p left\nA [rotate=90]\n");
    let dir = |t: &str, k: usize| {
        let id = n.find_tensor(&Name::plain(t)).unwrap();
        l.legs.iter().find(|g| g.tensor == id && g.slot == k).unwrap().dir
    };
    assert!(close(dir("W", 0), V3::xy(0.0, -1.0)));
    assert!(close(dir("W", 1), V3::xy(0.0, 1.0)));
    // `left` in A's frame, turned by A's rotation of 90 degrees.
    assert!(close(dir("A", 0), V3::xy(0.0, -1.0)));
}

#[test]
fn bond_routes() {
    let (_, l) = lay("A at (0, 0)\nB at (4, 0)\nA.a - B.a\nA.b - B.b\nA.c - B.c\nZ.l - Z.r\n");
    let parallel: Vec<_> = l.bonds.iter().filter(|b| b.a.tensor != b.b.tensor).collect();
    assert_eq!(parallel.len(), 3);
    let offsets: Vec<f64> = parallel.iter().map(|b| b.via.first().map_or(0.0, |p| p.y)).collect();
    assert!(offsets[0] < 0.0 && offsets[1].abs() < 1e-9 && offsets[2] > 0.0);
    let lp = l.bonds.iter().find(|b| b.a.tensor == b.b.tensor).unwrap();
    assert_eq!(lp.via.len(), 2);

    let (_, l) = lay("A at (0, 0)\nB at (4, 0)\nA - B [via=(2, 2)]\nA.#1 [leg-dir=up]\n");
    let b = &l.bonds[0];
    assert_eq!(b.via, vec![V3::xy(2.0, 2.0)]);
    assert!(close(b.a.dir.unwrap(), V3::xy(0.0, 1.0)));
    assert!(b.b.dir.is_none());
}
