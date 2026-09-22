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
fn legs_sharing_a_direction_spread_across_the_tensor() {
    let starts = |src: &str| -> Vec<(f64, f64)> {
        let (_, g) = build(src);
        g.lines
            .iter()
            .filter(|l| l.kind == LineKind::Leg)
            .map(|l| {
                let p = l.centerline.at(0.0).0;
                (p.x, p.y)
            })
            .collect()
    };
    // Width 4 less the corner radii: 3.6, in four parts of .9.
    let base = "A [width=4, corner-radius=.2, label=none]\nA: legs down, down, down, down\nA: leg up\n";
    let s = starts(base);
    for (k, x) in [-1.35, -0.45, 0.45, 1.35].into_iter().enumerate() {
        assert!(close(s[k].0, x) && close(s[k].1, 0.0), "{s:?}");
    }
    assert!(close(s[4].0, 0.0), "a leg alone stays at the centre");
    // An explicit offset takes a leg out of the spread; the others close up.
    let s = starts(&format!("{base}A.#2 [leg-offset=.1]\n"));
    for (k, x) in [(0, -1.2), (1, 0.1), (2, 0.0), (3, 1.2)] {
        assert!(close(s[k].0, x), "{s:?}");
    }
    // Every leg keeps the same visible length below the outline.
    let (_, g) = build(base);
    for l in g.lines.iter().filter(|l| l.kind == LineKind::Leg) {
        assert!(close(l.visible.1 - l.visible.0, 0.6));
    }
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
fn shape_names_belong_to_their_dimension() {
    let fails = |src: &str| {
        let net = parse(src).unwrap();
        let lay = layout(&net).unwrap();
        geometry(&net, &lay, &GeometryOptions::default(), &LabelSizes::new()).unwrap_err().to_string()
    };
    assert!(fails("A\nA [shape=box]\n").contains("3D shape, in a 2D scene"));
    assert!(fails("3d\nA\nA [shape=rect]\n").contains("2D shape, in a 3D scene"));
    assert!(parse("A\nA [rotate=(10, 0, 0)]\n").ok().and_then(|n| layout(&n).err()).is_some());
    // `dot` is in both.
    let net = parse("3d\nA\nA [shape=dot]\n").unwrap();
    assert!(geometry(&net, &layout(&net).unwrap(), &GeometryOptions::default(), &LabelSizes::new()).is_ok());
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

/// An arrowhead's tip and the midpoint of its base.
fn arrow_axis(line: &tnviz::LineGeom) -> (V3, V3) {
    let Some(tnviz::ArrowGeom::Flat { shape: arrow, .. }) = &line.arrow else { panic!("a flat arrow") };
    // A head alone: base corner, tip, base corner.
    let tip = arrow.pieces[1].start();
    let base = (arrow.pieces[0].start() + arrow.pieces[2].start()) * 0.5;
    (tip, base)
}

#[test]
fn arrows_point_along_their_line() {
    let (_, g) = build("A at (0, 0)\nB at (4, 0)\nA - B [arrow=forward]\n");
    let (tip, base) = arrow_axis(&g.lines[0]);
    assert!(tip.x > base.x && close(tip.y, 0.0));
    // Centred on the middle of the visible part, which is the middle here.
    assert!(close((tip.x + base.x) / 2.0, 2.0));

    let (_, g) = build("A at (0, 0)\nB at (4, 0)\nA - B [arrow=backward, arrow-pos=.25]\n");
    let (tip, base) = arrow_axis(&g.lines[0]);
    assert!(tip.x < base.x);
    let (from, to) = g.lines[0].visible;
    assert!(close((tip.x + base.x) / 2.0, from + 0.25 * (to - from)));

    // On a leg, forward is away from the tensor.
    let (_, g) = build("A: leg down\nleg [arrow=forward]\n");
    let (tip, base) = arrow_axis(&g.lines[0]);
    assert!(tip.y < base.y);
    assert!(g.lines[0].arrow.is_some());
}

#[test]
fn arrows_clear_a_label_on_their_tube() {
    let (_, g) = build(
        "A at (0, 0)\nB at (4, 0)\nA - B [tube, width=.4, label=$k$, arrow=forward, arrow-style=head]\n",
    );
    let label = &g.labels.iter().find(|l| l.on_line).unwrap();
    let (tip, base) = arrow_axis(&g.lines[0]);
    assert!(base.x > label.pos.x + label.size.0 / 2.0, "the arrow is past the label");
    assert!(tip.x > base.x);
    // No room: a warning, and no arrow.
    let (_, g) = build("A at (0, 0)\nB at (1.2, 0)\nA - B [tube, width=.6, arrow=forward]\n");
    assert!(g.lines[0].arrow.is_none());
    assert!(g.warnings.iter().any(|w| w.contains("arrow does not fit")), "{:?}", g.warnings);
}

#[test]
fn arrow_styles() {
    // Tubes end in a cone by default: its tip on the second tensor's
    // silhouette, and the tube cut at its base.
    let (_, g) = build("A at (0, 0)\nB at (4, 0)\nA - B [tube, arrow=forward]\n");
    let line = &g.lines[0];
    let Some(tnviz::ArrowGeom::Cone { outline, base, length, .. }) = &line.arrow else { panic!("a cone") };
    let tip = outline.pieces[1].start();
    assert!(close(tip.x, line.visible.1) && close(tip.x - base.x, *length));
    let tube_end = line.outline.as_ref().unwrap().sample(0.01).iter().map(|p| p.x).fold(f64::MIN, f64::max);
    assert!(close(tube_end, base.x), "the tube stops at the cone's base");

    // A label on the tube is centred on the shaft, clear of the cone.
    let (_, g) = build("A at (0, 0)\nB at (4, 0)\nA - B [tube, width=.4, label=$k$, arrow=forward]\n");
    let (from, _) = g.lines[0].visible;
    let Some(tnviz::ArrowGeom::Cone { base, .. }) = &g.lines[0].arrow else { panic!("a cone") };
    let label = g.labels.iter().find(|l| l.on_line).unwrap();
    assert!(close(label.pos.x, (from + base.x) / 2.0));

    // Beside: above a horizontal tube, parallel to it.
    let (_, g) = build("A at (0, 0)\nB at (4, 0)\nA - B [tube, arrow=forward, arrow-style=beside]\n");
    let Some(tnviz::ArrowGeom::Flat { shape, beside: true }) = &g.lines[0].arrow else { panic!("beside") };
    assert!(shape.sample(0.01).iter().all(|p| p.y > g.lines[0].width / 2.0));

    // Shaft: on the tube's axis, longer than a head.
    let (_, g) = build("A at (0, 0)\nB at (4, 0)\nA - B [tube, arrow=forward, arrow-style=shaft]\n");
    let Some(tnviz::ArrowGeom::Flat { shape, beside: false }) = &g.lines[0].arrow else { panic!("shaft") };
    let xs: Vec<f64> = shape.sample(0.01).iter().map(|p| p.x).collect();
    let span = xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
    assert!(span > 2.0 * 0.9 * 0.22);
}

#[test]
fn a_label_lies_over_a_shaft() {
    let (net, g) = build(
        "A at (0, 0)\nB at (4, 0)\nA - B [tube, width=.4, label=$k$, arrow=forward, arrow-style=shaft]\n",
    );
    let label = g.labels.iter().find(|l| l.on_line).unwrap();
    let Some(tnviz::ArrowGeom::Flat { shape, .. }) = &g.lines[0].arrow else { panic!("a shaft") };
    let xs: Vec<f64> = shape.sample(0.01).iter().map(|p| p.x).collect();
    let mid =
        (xs.iter().cloned().fold(f64::MIN, f64::max) + xs.iter().cloned().fold(f64::MAX, f64::min)) / 2.0;
    assert!(close(mid, label.pos.x), "the shaft stays centred under the label");
    // The label is drawn after the arrow.
    let parts: Vec<tnviz::Part> = tnviz::order(&net, &g).into_iter().map(|f| f.part).collect();
    let at = |p: tnviz::Part| parts.iter().position(|q| *q == p).unwrap();
    let k = g.labels.iter().position(|l| l.on_line).unwrap();
    assert!(at(tnviz::Part::Arrow(0)) < at(tnviz::Part::Label(k)));
}

#[test]
fn labels_at_the_ends_of_a_bond() {
    let mut sizes = LabelSizes::new();
    sizes.insert("b:k@1", 5.0, 4.0, 0.0);
    let (_, g) = build_with(
        "tensor A (k)\ntensor B (k)\nA at (0, 0)\nB at (4, 0)\nk [start-label=$i$, end-label=name]\n",
        &sizes,
    );
    let (from, to) = g.lines[0].visible;
    let at =
        |id: &str| g.labels.iter().find(|l| l.id == id).unwrap_or_else(|| panic!("no {id}: {:?}", g.labels));
    let (start, end) = (at("b:k@1"), at("b:k@2"));
    // .3em clear of the silhouettes, beside the bond on its upper side.
    let em = 10.0 / 28.45274;
    assert!(close(start.pos.x, from + 0.3 * em + start.size.0 / 2.0));
    assert!(close(end.pos.x, to - 0.3 * em - end.size.0 / 2.0));
    assert!(start.pos.y > 0.0 && end.pos.y > 0.0 && !start.on_line);
    assert_eq!(end.text, tnviz::LabelText::Math("$k$".into()));

    // A leg has one end, and `end-label` is for bonds only.
    let (_, g) = build("A: leg down\nleg [start-label=$s$]\n");
    assert_eq!(g.labels.iter().filter(|l| l.id.ends_with("@1")).count(), 1);
    assert!(tnviz::parse("A: leg down\nleg [end-label=$s$]\n").is_err());
}
