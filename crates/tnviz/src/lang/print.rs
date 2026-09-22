//! Print a network in canonical tnv: index syntax for the structure, then
//! groups, layout, and style rules.  Reading the output back gives the same
//! network, and printing that again gives the same text.

use std::fmt::Write;

use super::lower::VERSION;
use crate::model::{Dim, Direction, LayoutStmt, LegKey, Network, Relation, Selector, index_display};
use crate::value::{format_number, write_attrs, write_point};

pub fn to_tnv(net: &Network) -> String {
    let mut out = String::new();
    // Writing to a String cannot fail.
    write_network(&mut out, net).unwrap();
    out
}

fn write_network(o: &mut String, net: &Network) -> std::fmt::Result {
    writeln!(o, "tnv {VERSION}")?;
    let scene = net.scene();
    if scene.dim == Dim::Three {
        writeln!(o, "3d")?;
    }
    if let Some(d) = scene.spacing {
        writeln!(o, "spacing {}", format_number(d))?;
    }
    if let Some(light) = &scene.light {
        match light.as_slice() {
            [angle] => writeln!(o, "light {}", format_number(*angle))?,
            v => {
                o.push_str("light ");
                write_point(o, v)?;
                o.push('\n');
            }
        }
    }
    if let Some(cam) = &scene.camera {
        o.push_str("camera");
        if let Some((az, el)) = cam.angles {
            write!(o, " {} {}", format_number(az), format_number(el))?;
        }
        if !cam.attrs.is_empty() {
            o.push(' ');
            write_attrs(o, &cam.attrs)?;
        }
        o.push('\n');
    }

    let has_indices = net.indices().any(|(_, i)| !i.tags.is_empty() || i.dim.is_some());
    if has_indices {
        o.push('\n');
    }
    for (_, index) in net.indices() {
        if index.tags.is_empty() && index.dim.is_none() {
            continue;
        }
        write!(o, "index {}", index_display(&index.name, index.prime))?;
        if !index.tags.is_empty() {
            write!(o, " : {}", index.tags.iter().cloned().collect::<Vec<_>>().join(", "))?;
        }
        if let Some(dim) = index.dim {
            write!(o, " [dim={dim}]")?;
        }
        o.push('\n');
    }

    if net.tensors().len() > 0 {
        o.push('\n');
    }
    for (_, t) in net.tensors() {
        write!(o, "tensor {} (", t.name)?;
        for (k, slot) in t.slots.iter().enumerate() {
            if k > 0 {
                o.push_str(", ");
            }
            if let Some(label) = &slot.label {
                write!(o, "{label}=")?;
            }
            let index = net.index(slot.index);
            o.push_str(&index_display(&index.name, index.prime));
        }
        o.push_str(")\n");
    }

    if !net.groups().is_empty() {
        o.push('\n');
    }
    for g in net.groups() {
        let members: Vec<String> = g.members.iter().map(|t| net.tensor(*t).name.to_string()).collect();
        writeln!(o, "{}: {}", g.name, members.join(", "))?;
    }

    if !net.layout_statements().is_empty() {
        o.push('\n');
    }
    let name = |t| net.tensor(t).name.to_string();
    for l in net.layout_statements() {
        match l {
            LayoutStmt::At { tensor, pos } => {
                write!(o, "{} at ", name(*tensor))?;
                write_point(o, pos)?;
            }
            LayoutStmt::Relative { tensor, relation, anchor, distance } => {
                let rel = match relation {
                    Relation::RightOf => "right of",
                    Relation::LeftOf => "left of",
                    Relation::Above => "above",
                    Relation::Below => "below",
                };
                write!(o, "{} {rel} {}", name(*tensor), name(*anchor))?;
                if let Some(d) = distance {
                    write!(o, ", {}", format_number(*d))?;
                }
            }
            LayoutStmt::Row { tensors } => {
                let names: Vec<String> = tensors.iter().map(|t| name(*t)).collect();
                write!(o, "chain {}", names.join(", "))?;
            }
            LayoutStmt::Grid { base, rows, cols } => write!(o, "grid {base} {rows}x{cols}")?,
            LayoutStmt::Stack { groups } => write!(o, "stack {}", groups.join(", "))?,
            LayoutStmt::Tree { root, direction } => {
                write!(o, "tree {} ", name(*root))?;
                write_direction(o, direction)?;
            }
        }
        o.push('\n');
    }

    if !net.rules().is_empty() {
        o.push('\n');
    }
    for r in net.rules() {
        match &r.selector {
            Selector::Tensors => o.push('*'),
            Selector::Bonds => o.push('-'),
            Selector::Legs => o.push_str("leg"),
            Selector::OpenLegs => o.push_str("leg.open"),
            Selector::Tag(t) => write!(o, "tag:{t}")?,
            Selector::Name(p) => write!(o, "{p}")?,
            Selector::LegOf(p, key) => {
                write!(o, "{p}.")?;
                write_leg_key(o, key)?;
            }
            Selector::Tensor(t) => o.push_str(&name(*t)),
            Selector::Index(i) => {
                let index = net.index(*i);
                o.push_str(&index_display(&index.name, index.prime));
            }
            Selector::Slot(t, k) => {
                write!(o, "{}.", name(*t))?;
                match &net.tensor(*t).slots[*k].label {
                    Some(l) => o.push_str(l),
                    None => write!(o, "#{}", k + 1)?,
                }
            }
        }
        o.push(' ');
        write_attrs(o, &r.attrs)?;
        o.push('\n');
    }
    Ok(())
}

fn write_leg_key(o: &mut String, key: &LegKey) -> std::fmt::Result {
    match key {
        LegKey::Label(l) => write!(o, "{l}"),
        LegKey::Position(k) => write!(o, "#{k}"),
    }
}

fn write_direction(o: &mut String, d: &Direction) -> std::fmt::Result {
    match d {
        Direction::Word(w) | Direction::Axis(w) => o.push_str(w),
        Direction::Angle(a) => o.push_str(&format_number(*a)),
        Direction::Vector(v) => write_point(o, v)?,
    }
    Ok(())
}
