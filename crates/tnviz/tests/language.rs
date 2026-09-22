//! The examples of docs/language.md, read into networks.

use tnviz::{Attr, Dim, Name, Network, Selector, Value, parse, to_tnv};

fn net(src: &str) -> Network {
    parse(src).unwrap_or_else(|e| panic!("{e}\n---\n{src}"))
}

fn t(net: &Network, name: &str, subs: &[i64]) -> tnviz::TensorId {
    net.find_tensor(&Name::new(name, subs.to_vec())).unwrap_or_else(|| panic!("no tensor {name}{subs:?}"))
}

/// The bonds as sorted pairs of tensor names.
fn bonds(net: &Network) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = net
        .bonds()
        .map(|(_, i)| {
            let h = i.holders();
            let (a, b) = (net.tensor(h[0].0).name.to_string(), net.tensor(h[1].0).name.to_string());
            if a <= b { (a, b) } else { (b, a) }
        })
        .collect();
    out.sort();
    out
}

fn attr<'a>(attrs: &'a [Attr], key: &str) -> Option<&'a Value> {
    attrs.iter().find(|a| a.key == key).map(|a| &a.value)
}

/// Printing, reading back, and printing again gives the same text.
fn assert_round_trip(net: &Network) {
    let text = to_tnv(net);
    let again = parse(&text).unwrap_or_else(|e| panic!("{e}\n---\n{text}"));
    assert_eq!(to_tnv(&again), text);
    assert_eq!(bonds(&again), bonds(net));
}

#[test]
fn mps() {
    let n = net("chain A[1..6]\nA[*]: leg down\n");
    assert_eq!(n.tensors().len(), 6);
    assert_eq!(bonds(&n).len(), 5);
    assert_eq!(n.open_legs().count(), 6);
    let a3 = t(&n, "A", &[3]);
    let leg = n.tensor(a3).slots.iter().position(|s| n.index(s.index).is_open()).unwrap();
    assert_eq!(attr(&n.leg_style(a3, leg), "leg-dir"), Some(&Value::Word("down".into())));
    assert_round_trip(&n);
}

#[test]
fn peps() {
    let n = net("grid T 3x3\nT[*]: leg down-left\n");
    assert_eq!(n.tensors().len(), 9);
    assert_eq!(bonds(&n).len(), 12);
    assert_eq!(n.open_legs().count(), 9);
    let b = bonds(&n);
    assert!(b.contains(&("T[1,1]".into(), "T[1,2]".into())));
    assert!(b.contains(&("T[1,1]".into(), "T[2,1]".into())));
    assert_round_trip(&n);
}

#[test]
fn sandwich() {
    let n = net("ket: chain A[1..4]
         op:  chain W[1..4]
         bra: chain B[1..4]
         A[1..4] - W[1..4] - B[1..4]
         stack bra, op, ket
         op [shape=rect, color=teal]");
    assert_eq!(n.tensors().len(), 12);
    assert_eq!(bonds(&n).len(), 3 * 3 + 2 * 4);
    let w2 = n.tensor_style(t(&n, "W", &[2]));
    assert_eq!(attr(&w2, "color"), Some(&Value::Word("teal".into())));
    assert!(attr(&n.tensor_style(t(&n, "A", &[2])), "color").is_none());
    assert_round_trip(&n);
}

#[test]
fn tree() {
    let n = net("R - L[1], L[2]
         L[1] - X[1], X[2]
         L[2] - X[3], X[4]
         X[*]: leg down
         tree R down");
    assert_eq!(bonds(&n).len(), 6);
    assert_eq!(n.open_legs().count(), 4);
    assert_round_trip(&n);
}

#[test]
fn general_graph_with_hop() {
    let n = net("A at (0, 0);  B at (3, 0);  C at (1.5, 2)
         A - B - C - A
         D at (1.5, -1.5)
         C - D [hop]");
    assert_eq!(bonds(&n).len(), 4);
    let (a, b) = (t(&n, "C", &[]), t(&n, "D", &[]));
    let cd = n.bond_between(a, b).unwrap();
    assert_eq!(attr(&n.bond_style(cd), "crossing"), Some(&Value::Word("hop".into())));
    assert_round_trip(&n);
}

#[test]
fn scene_3d() {
    let n = net("3d, camera 35 25\ngrid T 2x2\nT[*]: leg +z\nT[*] [shape=box]\n");
    assert_eq!(n.scene().dim, Dim::Three);
    assert_eq!(n.scene().camera.as_ref().unwrap().angles, Some((35.0, 25.0)));
    let t11 = t(&n, "T", &[1, 1]);
    assert_eq!(attr(&n.tensor_style(t11), "shape"), Some(&Value::Word("box".into())));
    assert_round_trip(&n);
}

#[test]
fn index_syntax_mps_and_mpo() {
    let n = net("tnv 0.3
         index s[1..4] : Site
         index l[1..3] : Link [dim=8]
         tensor A[1] (s[1], l[1])
         tensor A[n] (s[n], l[n-1], l[n])  for n in 2..3
         tensor A[4] (s[4], l[3])
         tensor W[n] (s[n], s[n]', w[n-1], w[n])  for n in 2..3");
    // s[2], s[3], and w[2] are each shared by two tensors.
    assert_eq!(bonds(&n).len(), 3 + 2 + 1);
    let l2 = n.find_index(&Name::new("l", [2]), 0).unwrap();
    assert_eq!(n.index(l2).dim, Some(8));
    assert!(n.index(l2).tags.contains("Link"));
    assert!(n.find_index(&Name::new("s", [2]), 1).is_some());
    assert_round_trip(&n);
}

#[test]
fn labelled_legs_and_reuse() {
    let n = net("A.r - B.l [tube, label=$\\chi$]
         A - B
         A.x - B.x
         A: leg p down");
    // `A - B` reuses the labelled bond; `A.x - B.x` is a second bond.
    assert_eq!(bonds(&n).len(), 2);
    let a = t(&n, "A", &[]);
    let labels: Vec<_> = n.tensor(a).slots.iter().map(|s| s.label.clone().unwrap()).collect();
    assert_eq!(labels, ["r", "x", "p"]);
    let r = n.tensor(a).slots[0].index;
    assert_eq!(attr(&n.bond_style(r), "label"), Some(&Value::Math("$\\chi$".into())));
    assert_round_trip(&n);
}

#[test]
fn cascade_order() {
    let n = net("chain A[1..3]
         A[2] [color=red]
         * [color=gray, shape=orb]
         A[*] [color=blue]
         - [tube]
         tag:Link [width=.3]");
    let style = |k: i64| n.tensor_style(t(&n, "A", &[k]));
    // Name rules beat type rules whatever the order; later wins at a level.
    assert_eq!(attr(&style(1), "color"), Some(&Value::Word("blue".into())));
    assert_eq!(attr(&style(2), "color"), Some(&Value::Word("blue".into())));
    assert_eq!(attr(&style(2), "shape"), Some(&Value::Word("orb".into())));
    let (id, _) = n.bonds().next().unwrap();
    let b = n.bond_style(id);
    assert_eq!(attr(&b, "style"), Some(&Value::Word("tube".into())));
    assert_eq!(attr(&b, "width"), Some(&Value::Number(0.3)));
}

#[test]
fn errors_have_positions() {
    let cases = [
        ("chain A[1..3]\nA[1..2] - B[1..3]\n", "2:", "cannot pair"),
        ("B[*]: leg down\n", "1:", "matches no tensor"),
        ("A - B [label=$x]\n", "1:", "not closed"),
        ("tensor A (i)\ntensor B (i)\ntensor C (i)\n", "3:", "already joins two tensors"),
        ("stack nope\n", "1:", "not a group"),
        ("tnv 9.9\n", "1:", "asks for 9.9"),
        ("A [colour=red]\n", "1:", "unknown attribute `colour`"),
        ("A - B\nA [via=(1, 2)]\n", "2:", "`via` does not apply here"),
        ("A - B [label-pos=2]\n", "1:", "between 0 and 1"),
        ("A [shape=hexagon]\n", "1:", "one of rect"),
        ("A [bold]\n", "1:", "`bold` needs a value"),
        ("spacing 0\n", "1:", "spacing must be positive"),
        ("grid T 3\n", "1:", "size such as 3x3"),
        ("A: leg p sideways\n", "1:", "end of the statement"),
    ];
    for (src, pos, msg) in cases {
        let err = parse(src).expect_err(src).to_string();
        assert!(err.starts_with(pos) && err.contains(msg), "{src:?} gave {err:?}");
    }
}

#[test]
fn spacing_and_versions() {
    let n = net("tnv 0.2\nspacing 3\nchain A[1..2]\n");
    assert_eq!(n.scene().spacing, Some(3.0));
    assert!(to_tnv(&n).starts_with("tnv 0.3\nspacing 3\n"));
    assert_round_trip(&n);
}

#[test]
fn automatic_names_skip_used_ones() {
    let n = net("tensor A (_link[1])\ntensor B (_link[1])\nC - D\n");
    let (c, d) = (t(&n, "C", &[]), t(&n, "D", &[]));
    let bond = n.bond_between(c, d).unwrap();
    assert_eq!(n.index(bond).name, Name::new("_link", [2]));
    assert!(n.index(bond).tags.contains("Link"));
}

#[test]
fn checked_operations() {
    // Building through the API applies no syntax sugar and checks rules.
    let mut n = Network::new();
    let a = n.tensor_or_create(&Name::plain("A"));
    let i = n.index_or_create(&Name::plain("i"), 0);
    n.attach(a, i, None).unwrap();
    assert!(n.index(i).tags.is_empty());
    assert!(n.add_rule(Selector::Tensor(a), &[Attr::new("color", Value::Word("red".into()))]).is_ok());
    assert!(
        n.add_rule(Selector::Tensor(a), &[Attr::new("via", Value::Points(vec![vec![0.0, 0.0]]))]).is_err()
    );
    assert!(n.add_rule(Selector::Slot(a, 5), &[]).is_err());
    let text = to_tnv(&n);
    assert_eq!(to_tnv(&parse(&text).unwrap()), text);
}

#[test]
fn global_variables() {
    let n = net("let fresh = lime!55!green   // a comment\n\
                 let pale = @fresh!60!white; let fade = .6\n\
                 let chi = $\\chi$\n\
                 chain A[1..2]\n\
                 A[*] [color=@pale, opacity=@fade]\n\
                 A[1] - A[2] [label=@chi]\n\
                 A[2] [label=\"@fresh\"]\n");
    let a1 = n.tensor_style(t(&n, "A", &[1]));
    assert_eq!(attr(&a1, "color"), Some(&Value::Word("lime!55!green!60!white".into())));
    assert_eq!(attr(&a1, "opacity"), Some(&Value::Number(0.6)));
    // Inside a string, `@` is text.
    let a2 = n.tensor_style(t(&n, "A", &[2]));
    assert_eq!(attr(&a2, "label"), Some(&Value::Str("@fresh".into())));
    let (bond, _) = n.bonds().next().unwrap();
    assert_eq!(attr(&n.bond_style(bond), "label"), Some(&Value::Math("$\\chi$".into())));
    // Canonical tnv has no variables: they are expanded.
    let printed = to_tnv(&n);
    assert!(!printed.contains('@') || printed.contains("\"@fresh\""), "{printed}");
    assert!(printed.contains("lime!55!green!60!white"));

    let err = |src: &str| parse(src).unwrap_err().to_string();
    assert!(
        err("A [color=@nope]\n").starts_with("1:3: unknown variable `@nope`"),
        "{}",
        err("A [color=@nope]\n")
    );
    assert!(err("let x = 1\nlet x = 2\n").contains("already declared"));
    assert!(err("A [color=@x]\nlet x = red\n").contains("unknown variable"), "declared before use");
}

#[test]
fn scenes_in_3d_round_trip() {
    let src = "3d\nview x=(1, -0.35, 0.3), y=(0, 0.8, 0.6)\nlight world (1, 2, 3)\ngrid T 2x2\n\
               g: T[1..2, 1..2]\nT[*] [shape=prism, thickness=.5, rotate=(10, 20, 30)]\n\
               plane P under g [color=cyan, padding=.5]\nplane W at (0, 0, -1) [width=5]\nplane [opacity=.3]\n";
    let n = net(src);
    assert_eq!(n.planes().len(), 2);
    assert!(n.scene().light_world);
    let printed = to_tnv(&n);
    let again = net(&printed);
    assert_eq!(to_tnv(&again), printed, "{printed}");
    assert!(printed.contains("plane P under g") && printed.contains("plane [opacity=0.3]"), "{printed}");
    // Errors.
    assert!(parse("plane P\n").is_err());
    assert!(parse("3d\nview x=(1, 0, 0), y=(2, 0, 0)\n").is_err());
    assert!(parse("light world 30\n").is_err());
}
