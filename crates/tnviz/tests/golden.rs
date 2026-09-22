//! The TikZ output of every example in examples/tnv, compared byte for byte
//! with tests/golden/<name>.tikz, so that no change to the engine alters a
//! figure unnoticed.  After a deliberate change, regenerate the files with
//! `TNVIZ_BLESS=1 cargo test --test golden` and review the difference.

use std::path::Path;

use tnviz::{GeometryOptions, LabelSizes, geometry, layout, lighting, order, parse, tikz};

fn figure(src: &str) -> String {
    let net = parse(src).unwrap();
    let lay = layout(&net).unwrap();
    let opts = GeometryOptions::default();
    let geom = geometry(&net, &lay, &opts, &LabelSizes::new()).unwrap();
    let light = lighting(&net, &geom, &opts);
    tikz(&net, &geom, &light, &order(&net, &geom), &opts, src)
}

#[test]
fn examples_match_their_golden_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let examples = root.join("../../examples/tnv");
    let golden = root.join("tests/golden");
    let bless = std::env::var_os("TNVIZ_BLESS").is_some();
    let mut files: Vec<_> = std::fs::read_dir(&examples)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "tnv"))
        .collect();
    files.sort();
    assert!(!files.is_empty());
    let mut changed = Vec::new();
    for file in files {
        let name = file.file_stem().unwrap().to_string_lossy().to_string();
        let out = figure(&std::fs::read_to_string(&file).unwrap());
        let expected = golden.join(format!("{name}.tikz"));
        if bless {
            std::fs::write(&expected, &out).unwrap();
        } else if std::fs::read_to_string(&expected).ok().as_deref() != Some(out.as_str()) {
            changed.push(name);
        }
    }
    assert!(changed.is_empty(), "output changed for {changed:?}; if intended, rerun with TNVIZ_BLESS=1");
}
