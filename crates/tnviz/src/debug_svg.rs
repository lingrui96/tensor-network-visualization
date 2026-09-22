//! A debugging backend: a plain SVG of a network's geometry, to check
//! layout and geometry by eye.  Tensors and tubes are flat grey shapes,
//! lines are strokes, and labels are dashed boxes of their estimated or
//! measured size.  There is no lighting and no drawing order beyond bonds,
//! then tensors, then labels.

use std::fmt::Write;

use crate::geometry::{Anchor, Geometry, LineStyle, Path};
use crate::layout::V3;

const SCALE: f64 = 40.0;

/// The debugging picture of a network's geometry.
pub fn debug_svg(geom: &Geometry) -> String {
    let mut all: Vec<V3> = geom.tensors.iter().flat_map(|t| t.outline.sample(0.2)).collect();
    for l in &geom.lines {
        all.extend(l.outline.as_ref().unwrap_or(&l.centerline).sample(0.2));
    }
    let min_x = all.iter().map(|p| p.x).fold(f64::INFINITY, f64::min) - 0.5;
    let max_x = all.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max) + 0.5;
    let min_y = all.iter().map(|p| p.y).fold(f64::INFINITY, f64::min) - 0.5;
    let max_y = all.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max) + 0.5;
    let px = |p: V3| ((p.x - min_x) * SCALE, (max_y - p.y) * SCALE);
    let points = |path: &Path| -> String {
        path.sample(0.1)
            .iter()
            .map(|p| px(*p))
            .map(|(x, y)| format!("{x:.2},{y:.2}"))
            .collect::<Vec<_>>()
            .join(" ")
    };

    let mut s = String::new();
    let (w, h) = ((max_x - min_x) * SCALE, (max_y - min_y) * SCALE);
    // Writing to a String cannot fail.
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}">"#);
    let _ = writeln!(s, r##"<rect width="100%" height="100%" fill="#fff"/>"##);
    for l in &geom.lines {
        match (&l.outline, l.style) {
            (Some(outline), LineStyle::Tube) => {
                let _ = writeln!(
                    s,
                    r##"<polygon points="{}" fill="#b8bec8" stroke="#555" stroke-width="1"/>"##,
                    points(outline)
                );
            }
            _ => {
                let _ = writeln!(
                    s,
                    r##"<polyline points="{}" fill="none" stroke="#333" stroke-width="{:.2}"/>"##,
                    points(&l.centerline),
                    (l.width * SCALE).max(1.0)
                );
            }
        }
    }
    for t in &geom.tensors {
        let _ = writeln!(
            s,
            r##"<polygon points="{}" fill="#4a63c8" stroke="#1f2f73" stroke-width="1.2"/>"##,
            points(&t.outline)
        );
    }
    for l in &geom.labels {
        let (w, h, d) = l.size;
        // The box's centre from its anchor.
        let center = match &l.anchor {
            Anchor::Center => l.pos,
            Anchor::Toward(dir) => {
                let (hx, hy) = (w / 2.0, (h + d) / 2.0);
                let t = [hx / dir.x.abs().max(1e-12), hy / dir.y.abs().max(1e-12)]
                    .into_iter()
                    .fold(f64::INFINITY, f64::min);
                l.pos - *dir * t
            }
        };
        let (cx, cy) = px(center);
        let _ = writeln!(
            s,
            r##"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="none" stroke="#c0392b" stroke-dasharray="2,2" transform="rotate({:.2} {cx:.2} {cy:.2})"/>"##,
            cx - w / 2.0 * SCALE,
            cy - (h + d) / 2.0 * SCALE,
            w * SCALE,
            (h + d) * SCALE,
            -l.angle
        );
        let name = l.id.split_once(':').map_or(l.id.as_str(), |(_, n)| n);
        let _ = writeln!(
            s,
            r##"<text x="{cx:.2}" y="{:.2}" font-family="sans-serif" font-size="8" text-anchor="middle" fill="#c0392b" transform="rotate({:.2} {cx:.2} {cy:.2})">{name}</text>"##,
            cy + 3.0,
            -l.angle
        );
    }
    s.push_str("</svg>\n");
    s
}
