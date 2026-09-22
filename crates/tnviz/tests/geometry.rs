//! The geometry stage (docs/language.md, section 8).

use tnviz::{
    Anchor, Geometry, GeometryOptions, LabelSizes, LineKind, LineStyle, Name, Network, V3, geometry, layout,
    parse,
};

fn build_with(src: &str, sizes: &LabelSizes) -> (Network, Geometry) {
    let net = parse(src).unwrap_or_else(|e| panic!("{e}"));
    let lay = layout(&net).unwrap();
    let geom = geometry(&net, &lay, &GeometryOptions::default(), sizes).unwrap_or_else(|e| panic!("{e}"));
    (net, geom)
}

fn build(src: &str) -> (Network, Geometry) {
    build_with(src, &LabelSizes::new())
}

fn tensor<'a>(net: &Network, g: &'a Geometry, name: &str) -> &'a tnviz::TensorGeom {
    &g.tensors[net.find_tensor(&Name::plain(name)).unwrap().0]
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn shapes_take_default_sizes_and_grow_for_labels() {
    let mut sizes = LabelSizes::new();
    // A label 2cm wide in TeX points.
    sizes.insert("t:B", 56.9, 7.0, 2.0);
    let (net, g) = build_with(
        "A at (0, 0)\nB at (4, 0)\nC at (8, 0)\nC [shape=diamond, width=1, height=.5, corner-radius=0]\n",
        &sizes,
    );
    let a = tensor(&net, &g, "A");
    assert!(close(a.width, 1.1) && close(a.height, 0.75));
    let b = tensor(&net, &g, "B");
    assert!(b.width > 2.0 && close(b.height, 0.75));
    let c = tensor(&net, &g, "C");
    assert!(close(c.width, 1.0) && close(c.corner_radius, 0.0));
    let right = c.outline.ray_distance(c.center, V3::xy(1.0, 0.0)).unwrap();
    assert!(close(right, 0.5));
    assert_eq!(g.labels.iter().filter(|l| l.measured).count(), 1);
}

#[test]
fn corner_radius_is_clamped_silently() {
    let (net, g) = build("A\nA [corner-radius=5]\n");
    let a = tensor(&net, &g, "A");
    assert!(close(a.corner_radius, 0.375));
    assert!(g.warnings.is_empty());
}

#[test]
fn legs_reach_past_the_outline() {
    let (net, g) = build("A: leg down\nA.#1 [leg-length=1]\n");
    let a = tensor(&net, &g, "A");
    let leg = g.lines.iter().find(|l| l.kind == LineKind::Leg).unwrap();
    assert!(close(leg.centerline.length(), a.height / 2.0 + 1.0));
    assert!(close(leg.visible.0, a.height / 2.0) && close(leg.visible.1, leg.centerline.length()));
}

#[test]
fn bonds_hide_inside_their_tensors() {
    let (net, g) = build("A at (0, 0)\nB at (3, 0)\nA - B\n");
    let bond = &g.lines[0];
    assert!(close(bond.centerline.length(), 3.0));
    let half = tensor(&net, &g, "A").width / 2.0;
    assert!(close(bond.visible.0, half) && close(bond.visible.1, 3.0 - half));
    // A line is 0.09em wide: 0.9pt at 10pt, in centimetres.
    assert!(close(bond.width, 0.9 / 28.45274));
}

#[test]
fn ports_run_straight_before_turning() {
    let (net, g) = build("A at (0, 0)\nB at (4, 0)\nA.r - B.l [via=(2, 2)]\nA.r [leg-dir=up]\n");
    let a = tensor(&net, &g, "A");
    let bond = &g.lines[0];
    // The first piece leaves A straight up, at least past the stub.
    let (start, dir) = bond.centerline.at(0.0);
    assert!(close(start.x, 0.0) && close(dir.x, 0.0) && dir.y > 0.99);
    let first = &bond.centerline.pieces[0];
    assert!(first.length() > a.height / 2.0);
    assert!(bond.centerline.pieces.iter().any(|p| matches!(p, tnviz::Piece::Arc { .. })));
}

#[test]
fn tubes_have_closed_outlines() {
    let (_, g) = build("A at (0, 0)\nB at (4, 0)\nA - B [tube, via=(2, 1)]\n");
    let bond = &g.lines[0];
    assert_eq!(bond.style, LineStyle::Tube);
    let outline = bond.outline.as_ref().unwrap();
    let n = outline.pieces.len();
    for k in 0..n {
        assert!((outline.pieces[k].end() - outline.pieces[(k + 1) % n].start()).norm() < 1e-9);
    }
}

#[test]
fn parallel_bonds_and_loops() {
    let (net, g) = build("A at (0, 0)\nB at (4, 0)\nA.a - B.a\nA.b - B.b\nZ at (0, 5)\nZ.l - Z.r\n");
    let mids: Vec<f64> =
        g.lines[..2].iter().map(|l| l.centerline.at(l.centerline.length() / 2.0).0.y).collect();
    // The fillet at the route point pulls the midpoint slightly inwards from
    // the gap of 0.3.
    assert!(mids[0] < 0.0 && mids[1] > 0.0 && close(mids[0], -mids[1]));
    assert!(mids[1] - mids[0] > 0.2 && mids[1] - mids[0] <= 0.3);
    let z = tensor(&net, &g, "Z");
    let lp = &g.lines[2];
    let top = lp.centerline.sample(0.05).iter().map(|p| p.y).fold(f64::MIN, f64::max);
    assert!(top > z.center.y + z.height / 2.0 + 0.2);
}

#[test]
fn crossings_hop_when_asked() {
    let src = "A at (0, 0)\nB at (4, 0)\nC at (2, -2)\nD at (2, 2)\nA - B\nC - D [tube, hop]\n";
    let (_, g) = build(src);
    assert_eq!(g.crossings.len(), 1);
    assert!(g.crossings[0].hopped);
    let straight = 4.0;
    assert!(g.lines[1].centerline.length() > straight + 0.3);
    // Without hop: the same crossing, left alone.
    let (_, g) = build(&src.replace(", hop", ""));
    assert_eq!(g.crossings.len(), 1);
    assert!(!g.crossings[0].hopped);
    assert!(close(g.lines[1].centerline.length(), straight));
}

#[test]
fn bond_labels() {
    let src = "A at (4, 0)\nB at (0, 0)\nA - B [tube, width=.5, label=$\\chi$]\nC at (0, -3)\nD at (4, -3)\nC - D [label=$k$]\n";
    let (_, g) = build(src);
    let on = g.labels.iter().find(|l| l.id == "b:_link[1]").unwrap();
    // The bond runs right to left; the text is still upright.
    assert!(close(on.angle, 0.0) && close(on.pos.x, 2.0) && close(on.pos.y, 0.0));
    assert_eq!(on.anchor, Anchor::Center);
    let beside = g.labels.iter().find(|l| l.id == "b:_link[2]").unwrap();
    assert!(beside.pos.y > -3.0);
    assert!(matches!(beside.anchor, Anchor::Toward(d) if d.y < -0.99));
    assert!(g.warnings.is_empty(), "{:?}", g.warnings);
}

#[test]
fn unsupported_scenes() {
    let net = parse("3d\nA\n").unwrap();
    let lay = layout(&net).unwrap();
    assert!(geometry(&net, &lay, &GeometryOptions::default(), &LabelSizes::new()).is_err());
    let net = parse("A\nA [shape=box]\n").unwrap();
    let lay = layout(&net).unwrap();
    assert!(geometry(&net, &lay, &GeometryOptions::default(), &LabelSizes::new()).is_err());
}

#[test]
fn tube_labels_go_beside_when_they_do_not_fit() {
    // A default tube (0.22) is thinner than a 10pt label.
    let (_, g) = build("A at (0, 0)\nB at (3, 0)\nA - B [tube, label=$\\chi$]\n");
    let label = &g.labels.iter().find(|l| l.id.starts_with("b:")).unwrap();
    assert!(matches!(label.anchor, Anchor::Toward(_)));
    assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    // Asked to go on the tube anyway: it does, with a warning.
    let (_, g) = build("A at (0, 0)\nB at (3, 0)\nA - B [tube, label=$\\chi$, label-placement=on]\n");
    let label = &g.labels.iter().find(|l| l.id.starts_with("b:")).unwrap();
    assert_eq!(label.anchor, Anchor::Center);
    assert_eq!(g.warnings.len(), 1);
}
