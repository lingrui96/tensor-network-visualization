//! 3D scenes (docs/language.md, section 11).

use tnviz::{
    ArrowGeom, Geometry, GeometryOptions, LabelSizes, LineKind, Network, Part, Patch, TensorId, V3, geometry,
    layout, lighting, order, parse, tikz,
};

fn build(src: &str) -> (Network, Geometry) {
    let net = parse(src).unwrap_or_else(|e| panic!("{e}"));
    let lay = layout(&net).unwrap_or_else(|e| panic!("{e}"));
    let geom = geometry(&net, &lay, &GeometryOptions::default(), &LabelSizes::new())
        .unwrap_or_else(|e| panic!("{e}"));
    (net, geom)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn layout_in_3d_stacks_along_z_and_legs_point_along_z() {
    let net = parse("3d\nket: chain A[1..2]\nbra: chain B[1..2]\nstack bra, ket\nA[*]: leg\n").unwrap();
    let lay = layout(&net).unwrap();
    let a = net.find_tensor(&tnviz::Name::new("A", vec![1])).unwrap();
    let b = net.find_tensor(&tnviz::Name::new("B", vec![1])).unwrap();
    // The first group on top: bra above ket, along z.
    let d = lay.pos(b) - lay.pos(a);
    assert!(close(d.x, 0.0) && close(d.y, 0.0) && close(d.z, 2.0), "{d:?}");
    // A leg of prime level 0 points down z.
    assert!((lay.legs[0].dir - V3::new(0.0, 0.0, -1.0)).norm() < 1e-9);
    // In 2D, the same stack runs along -y.
    let net = parse("ket: chain A[1..2]\nbra: chain B[1..2]\nstack bra, ket\n").unwrap();
    let lay = layout(&net).unwrap();
    let d = lay.pos(net.find_tensor(&tnviz::Name::new("B", vec![1])).unwrap())
        - lay.pos(net.find_tensor(&tnviz::Name::new("A", vec![1])).unwrap());
    assert!(close(d.y, 2.0) && close(d.z, 0.0));
}

#[test]
fn the_view_from_above_shows_the_2d_picture() {
    let (_, g) = build("3d, camera 0 90\nA at (0, 0)\nB at (3, 1)\nA [shape=sphere]\nB [shape=box]\n");
    assert!((g.tensors[1].center - V3::xy(3.0, 1.0)).norm() < 1e-9);
    // A box seen from above: its silhouette is its 2D cross-section.
    let right = g.tensors[1].outline.ray_distance(V3::xy(3.0, 1.0), V3::xy(1.0, 0.0)).unwrap();
    assert!(close(right, 0.55));
    // One face shows, the top.
    let faces = g.tensors[1].patches.iter().filter(|p| matches!(p, Patch::Face { .. })).count();
    assert_eq!(faces, 1);
    // `view` with two axes gives the same.
    let (_, h) = build(
        "3d, view x=(1, 0, 0), y=(0, 1, 0)\nA at (0, 0)\nB at (3, 1)\nA [shape=sphere]\nB [shape=box]\n",
    );
    assert!((h.tensors[1].center - g.tensors[1].center).norm() < 1e-9);
}

#[test]
fn bonds_stop_at_the_surfaces_of_their_tensors() {
    let (_, g) =
        build("3d, camera 0 90\nA at (0, 0)\nB at (3, 0)\nA [shape=sphere]\nB [shape=sphere]\nA - B\n");
    let line = &g.lines[0];
    let (start, _) = line.centerline.at(0.0);
    let (end, _) = line.centerline.at(line.centerline.length());
    assert!(close(start.x, 0.4) && close(end.x, 2.6), "{start:?} {end:?}");
    assert!(!line.spans.is_empty());
    // Hops are 2D only.
    let (_, g) = build("3d\nA at (0, 0)\nB at (3, 0)\nA - B [hop]\n");
    assert!(g.warnings.iter().any(|w| w.contains("2D only")));
}

#[test]
fn nearer_things_are_drawn_later() {
    // A sphere in front of a bond that passes behind it.
    let src = "3d, camera 0 0\nA at (-2, 0, 0)\nB at (2, 0, 0)\nC at (0, -1, 0)\nA - B\n";
    let (net, g) = build(src);
    let parts: Vec<Part> = order(&net, &g).into_iter().map(|f| f.part).collect();
    let c = parts.iter().position(|p| *p == Part::Tensor(TensorId(2))).unwrap();
    // The span of the bond behind C comes before C.
    let behind = g.lines[0]
        .spans
        .iter()
        .position(|s| {
            let mid = g.lines[0].centerline.at((s.from + s.to) / 2.0).0;
            mid.x.abs() < 0.2
        })
        .unwrap();
    let span = parts.iter().position(|p| *p == Part::Span(0, behind)).unwrap();
    assert!(span < c, "{parts:?}");
}

#[test]
fn planes_divide_the_order_into_layers() {
    let src = "3d\ngrid A 2x2\ngrid B 2x2\nket: A[1..2, 1..2]\nbra: B[1..2, 1..2]\nstack bra, ket\n\
               B[1, 1] at (0, 0, 0)\nA[1..2, 1..2] - B[1..2, 1..2]\n- [tube]\nplane P under bra\n";
    let (net, g) = build(src);
    assert_eq!(g.planes.len(), 1);
    let parts: Vec<Part> = order(&net, &g).into_iter().map(|f| f.part).collect();
    let at = |p: Part| parts.iter().position(|q| *q == p).unwrap();
    let plane = at(Part::Plane(0));
    // The plane passes through the upper layer's centres: each upper tensor
    // is drawn whole behind it, then its part in front after it.  The lower
    // layer is behind the plane.
    let (a, b) = (
        net.find_tensor(&tnviz::Name::new("A", vec![1, 1])).unwrap(),
        net.find_tensor(&tnviz::Name::new("B", vec![1, 1])).unwrap(),
    );
    assert!(at(Part::Tensor(a)) < plane);
    assert!(at(Part::Tensor(b)) < plane && plane < at(Part::TensorFront(b)));
    assert!(g.tensors[b.0].front.is_some() && g.tensors[a.0].front.is_none());
    // A bond lying in the plane is split along its length.
    assert!(g.lines.iter().any(|l| l.fronts.iter().any(|f| f.is_some())));
    // A bond between the layers ends under the upper tensors, below the
    // plane through their centres; a plane halfway between the layers cuts
    // it in two.
    let (_, g) = build(&format!("{src}plane W at (0.5, 0.5, -1) [width=6, height=6]\n"));
    let crosses = g.lines.iter().any(|l| {
        l.kind == LineKind::Bond
            && l.spans.iter().any(|s| s.layer == 0)
            && l.spans.iter().any(|s| s.layer > 0)
    });
    assert!(crosses);
    // Planes need a 3D scene.
    let net = parse("grid A 2x2\ng: A[1..2, 1..2]\nplane P under g\n").unwrap();
    assert!(geometry(&net, &layout(&net).unwrap(), &GeometryOptions::default(), &LabelSizes::new()).is_err());
}

#[test]
fn cones_are_3d_arrows_too() {
    let (_, g) = build("3d\nA at (0, 0)\nB at (3, 0)\nA - B [tube, arrow=forward]\n");
    assert!(matches!(g.lines[0].arrow, Some(ArrowGeom::Cone { .. })));
}

#[test]
fn figures_compile() {
    let src = "3d\nlet c = lime!60!white\ngrid T 2x2\nT[*]: leg\nT[*] [shape=octahedron, color=@c]\n\
               - [tube]\nleg [tube]\nX at (4, 0, 0)\nX [shape=prism]\nY at (4, 2, 0)\nY [shape=sphere]\n\
               g: T[1..2, 1..2]\nplane P under g [opacity=.25]\n";
    let (net, g) = build(src);
    let opts = GeometryOptions::default();
    let light = lighting(&net, &g, &opts);
    let code = tikz(&net, &g, &light, &order(&net, &g), &opts, src);
    assert!(code.starts_with("\\tnvRuntime{3}%"));
    assert!(code.contains("\\tnvShade") && code.contains("\\tnvFill"));
}
