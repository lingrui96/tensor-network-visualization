//! `tnviz`: check, format, and lay out tnv files.

use std::fmt::Write;
use std::process::ExitCode;

use tnviz::layout::{Layout, V3};
use tnviz::{LayoutOptions, Network};

const USAGE: &str = "\
usage: tnviz <command> <file> [options]

commands:
  check                 read a tnv file and summarize the network
  fmt                   print the network in canonical tnv
  layout [--svg FILE]   print tensor positions; --svg also draws a plain
                        debugging picture of the layout";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage) => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Message(m)) => {
            eprintln!("{m}");
            ExitCode::FAILURE
        }
    }
}

enum Failure {
    Usage,
    Message(String),
}

fn run(args: &[String]) -> Result<(), Failure> {
    let (command, path, rest) = match args {
        [command, path, rest @ ..] => (command.as_str(), path, rest),
        _ => return Err(Failure::Usage),
    };
    let src = std::fs::read_to_string(path)
        .map_err(|e| Failure::Message(format!("tnviz: cannot read {path}: {e}")))?;
    let net = tnviz::parse(&src).map_err(|e| Failure::Message(format!("{path}:{e}")))?;
    match (command, rest) {
        ("check", []) => {
            println!(
                "{path}: {} tensors, {} bonds, {} open legs",
                net.tensors().len(),
                net.bonds().count(),
                net.open_legs().count()
            );
        }
        ("fmt", []) => print!("{}", tnviz::to_tnv(&net)),
        ("layout", rest) => {
            let svg = match rest {
                [] => None,
                [flag, file] if flag == "--svg" => Some(file),
                _ => return Err(Failure::Usage),
            };
            let lay = tnviz::layout(&net, &LayoutOptions::default())
                .map_err(|e| Failure::Message(format!("{path}: {e}")))?;
            for (id, t) in net.tensors() {
                let p = lay.pos(id);
                println!("{:<12} {:>8.3} {:>8.3} {:>8.3}", t.name.to_string(), p.x, p.y, p.z);
            }
            if let Some(file) = svg {
                std::fs::write(file, debug_svg(&net, &lay))
                    .map_err(|e| Failure::Message(format!("tnviz: cannot write {file}: {e}")))?;
            }
        }
        _ => return Err(Failure::Usage),
    }
    Ok(())
}

/// A plain picture for checking layouts: dots, straight segments, and
/// names.  Not a renderer.
fn debug_svg(net: &Network, lay: &Layout) -> String {
    const SCALE: f64 = 40.0;
    const PORT: f64 = 0.45;
    let mut points: Vec<V3> = lay.tensors.iter().map(|t| t.pos).collect();
    for b in &lay.bonds {
        points.extend(&b.via);
    }
    for l in &lay.legs {
        points.push(lay.pos(l.tensor) + l.dir * l.length);
    }
    let min_x = points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min) - 1.0;
    let max_x = points.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let min_y = points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min) - 1.0;
    let max_y = points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let px = |p: V3| ((p.x - min_x) * SCALE, (max_y - p.y) * SCALE);

    let mut s = String::new();
    let (w, h) = ((max_x - min_x) * SCALE, (max_y - min_y) * SCALE);
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}">"#);
    let _ = writeln!(s, r##"<rect width="100%" height="100%" fill="#fff"/>"##);
    for b in &lay.bonds {
        let mut path = vec![lay.pos(b.a.tensor)];
        if let Some(d) = b.a.dir {
            path.push(lay.pos(b.a.tensor) + d * PORT);
        }
        path.extend(&b.via);
        if let Some(d) = b.b.dir {
            path.push(lay.pos(b.b.tensor) + d * PORT);
        }
        path.push(lay.pos(b.b.tensor));
        let pts: Vec<String> = path.iter().map(|p| px(*p)).map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
        let _ = writeln!(
            s,
            r##"<polyline points="{}" fill="none" stroke="#555" stroke-width="2"/>"##,
            pts.join(" ")
        );
    }
    for l in &lay.legs {
        let (x1, y1) = px(lay.pos(l.tensor));
        let (x2, y2) = px(lay.pos(l.tensor) + l.dir * l.length);
        let _ = writeln!(
            s,
            r##"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="#555" stroke-width="2"/>"##
        );
    }
    for (id, t) in net.tensors() {
        let (x, y) = px(lay.pos(id));
        let _ = writeln!(s, r##"<circle cx="{x:.1}" cy="{y:.1}" r="11" fill="#3b5bdb"/>"##);
        let _ = writeln!(
            s,
            r##"<text x="{x:.1}" y="{:.1}" font-family="sans-serif" font-size="9" text-anchor="middle" fill="#fff">{}</text>"##,
            y + 3.0,
            t.name
        );
    }
    s.push_str("</svg>\n");
    s
}
