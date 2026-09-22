//! A debugging backend: a plain SVG of a placement, to check layouts by eye.
//!
//! It draws tensors as dots and bonds as straight segments.  It is not the
//! geometry of docs/language.md, section 8: parallel bonds, loops, ports,
//! and open legs are drawn with fixed approximate sizes, because shapes are
//! not known here.

use std::fmt::Write;

use crate::layout::{Placement, Route, V3};
use crate::model::Network;

const SCALE: f64 = 40.0;
const PORT: f64 = 0.45;
const LEG: f64 = 0.6;
const FAN: f64 = 0.35;
const LOOP: f64 = 0.7;

/// The debugging picture of a placement.
pub fn debug_svg(net: &Network, lay: &Placement) -> String {
    let paths: Vec<Vec<V3>> = lay
        .bonds
        .iter()
        .map(|b| {
            let (pa, pb) = (lay.pos(b.a.tensor), lay.pos(b.b.tensor));
            let mut path = vec![pa];
            if let Some(d) = b.a.dir {
                path.push(pa + d * PORT);
            }
            match &b.route {
                Route::Direct => {}
                Route::Via(points) => path.extend(points),
                Route::Parallel { k, n } => {
                    let side = *k as f64 - (*n - 1) as f64 / 2.0;
                    let normal = V3::xy(-(pb - pa).y, (pb - pa).x).unit().unwrap_or(V3::xy(0.0, 1.0));
                    path.push((pa + pb) * 0.5 + normal * (side * FAN));
                }
                Route::Loop { k } => {
                    let r = LOOP * (*k + 1) as f64;
                    path.extend([pa + V3::xy(-0.6 * r, 1.4 * r), pa + V3::xy(0.6 * r, 1.4 * r)]);
                }
            }
            if let Some(d) = b.b.dir {
                path.push(pb + d * PORT);
            }
            path.push(pb);
            path
        })
        .chain(lay.legs.iter().map(|l| vec![lay.pos(l.tensor), lay.pos(l.tensor) + l.dir * LEG]))
        .collect();

    let all: Vec<V3> = lay.tensors.iter().map(|t| t.pos).chain(paths.iter().flatten().copied()).collect();
    let min_x = all.iter().map(|p| p.x).fold(f64::INFINITY, f64::min) - 1.0;
    let max_x = all.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let min_y = all.iter().map(|p| p.y).fold(f64::INFINITY, f64::min) - 1.0;
    let max_y = all.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max) + 1.0;
    let px = |p: V3| ((p.x - min_x) * SCALE, (max_y - p.y) * SCALE);

    let mut s = String::new();
    let (w, h) = ((max_x - min_x) * SCALE, (max_y - min_y) * SCALE);
    // Writing to a String cannot fail.
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}">"#);
    let _ = writeln!(s, r##"<rect width="100%" height="100%" fill="#fff"/>"##);
    for path in &paths {
        let pts: Vec<String> = path.iter().map(|p| px(*p)).map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
        let _ = writeln!(
            s,
            r##"<polyline points="{}" fill="none" stroke="#555" stroke-width="2"/>"##,
            pts.join(" ")
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
