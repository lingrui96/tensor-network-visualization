//! Turn statements into a network.
//!
//! Statements run in order.  Structural statements (connections, legs,
//! groups) see the tensors that exist when they run, while style rules are
//! stored with their selectors and match whenever styles are resolved.

use std::collections::HashMap;

use super::ast::*;
use crate::error::{Error, Pos, Result};
use crate::model::{IndexId, LayoutStmt, LegKey, Network, Selector, TensorId};
use crate::name::{Name, NamePattern, SubPattern};
use crate::value::{Attr, Value};

/// The tnv format version this crate writes.
pub const VERSION: &str = "0.3";
/// Versions this crate reads.
const READS: [&str; 2] = ["0.2", "0.3"];

/// Base names of indices created by the simple syntax.  The leading
/// underscore keeps them apart from hand-written names.
const AUTO_LINK: &str = "_link";
const AUTO_SITE: &str = "_site";

pub fn lower(stmts: &[Stmt]) -> Result<Network> {
    let mut net = Network::new();
    let mut auto = Auto::default();
    for stmt in stmts {
        statement(&mut net, &mut auto, stmt).map_err(|e| e.or_at(stmt.pos))?;
    }
    Ok(net)
}

/// Counters for the index names that the simple syntax creates.
#[derive(Default)]
struct Auto {
    links: i64,
    sites: i64,
}

/// A fresh index `base[k]`, skipping names already in use.
fn fresh_index(net: &mut Network, base: &str, counter: &mut i64, tag: &str) -> IndexId {
    loop {
        *counter += 1;
        let name = Name::new(base, [*counter]);
        if net.find_index(&name, 0).is_none() {
            let id = net.index_or_create(&name, 0);
            net.index_mut(id).tags.insert(tag.into());
            return id;
        }
    }
}

/// `a - b` in the simple syntax.  Without leg labels, an existing bond
/// between the two tensors is reused, so `A - B` twice is one bond; labelled
/// legs are matched by label, and new legs are created as needed.
fn connect(
    net: &mut Network,
    auto: &mut Auto,
    a: TensorId,
    la: Option<&str>,
    b: TensorId,
    lb: Option<&str>,
) -> Result<IndexId> {
    let sa = la.and_then(|l| net.slot_by_label(a, l));
    let sb = lb.and_then(|l| net.slot_by_label(b, l));
    match (sa, sb) {
        (Some(sa), Some(sb)) => {
            let (ia, ib) = (net.tensor(a).slots[sa].index, net.tensor(b).slots[sb].index);
            if ia == ib {
                Ok(ia)
            } else {
                Err(Error::new(format!(
                    "{}.{} and {}.{} are already used by other bonds",
                    net.tensor(a).name,
                    la.unwrap_or_default(),
                    net.tensor(b).name,
                    lb.unwrap_or_default()
                )))
            }
        }
        (Some(sa), None) => {
            let index = net.tensor(a).slots[sa].index;
            net.attach(b, index, lb.map(str::to_string))?;
            Ok(index)
        }
        (None, Some(sb)) => {
            let index = net.tensor(b).slots[sb].index;
            net.attach(a, index, la.map(str::to_string))?;
            Ok(index)
        }
        (None, None) => {
            if la.is_none()
                && lb.is_none()
                && let Some(existing) = net.bond_between(a, b)
            {
                return Ok(existing);
            }
            let index = fresh_index(net, AUTO_LINK, &mut auto.links, "Link");
            net.attach(a, index, la.map(str::to_string))?;
            net.attach(b, index, lb.map(str::to_string))?;
            Ok(index)
        }
    }
}

/// `A: leg` in the simple syntax: a new open leg, tagged `Site`.
fn open_leg(net: &mut Network, auto: &mut Auto, t: TensorId, label: Option<String>) -> Result<usize> {
    let index = fresh_index(net, AUTO_SITE, &mut auto.sites, "Site");
    net.attach(t, index, label)
}

type Env = HashMap<String, i64>;

/// Run `f` once, or once per value of the `for` variable.
fn each(clause: &Option<ForClause>, mut f: impl FnMut(&Env) -> Result<()>) -> Result<()> {
    match clause {
        None => f(&Env::new()),
        Some(c) => {
            for v in c.from..=c.to {
                f(&Env::from([(c.var.clone(), v)]))?;
            }
            Ok(())
        }
    }
}

fn eval(e: &Expr, env: &Env, pos: Pos) -> Result<i64> {
    match e {
        Expr::Int(n) => Ok(*n),
        Expr::Var(v, off) => {
            env.get(v).map(|x| x + off).ok_or_else(|| Error::at(pos, format!("`{v}` is not a loop variable")))
        }
    }
}

/// Evaluate loop variables, leaving a pattern.
fn pattern(name: &NameAst, env: &Env) -> Result<NamePattern> {
    let subscripts = name
        .subs
        .iter()
        .map(|s| {
            Ok(match s {
                Sub::Any => SubPattern::Any,
                Sub::Expr(e) => SubPattern::Exact(eval(e, env, name.pos)?),
                Sub::Range(a, b) => SubPattern::Range(eval(a, env, name.pos)?, eval(b, env, name.pos)?),
            })
        })
        .collect::<Result<_>>()?;
    Ok(NamePattern { base: name.base.clone(), subscripts, prime: name.prime })
}

/// The names a pattern stands for, in order: ranges enumerate their values,
/// and `*` matches existing tensors.
fn tensor_names(net: &Network, name: &NameAst, env: &Env) -> Result<Vec<Name>> {
    if name.prime != 0 {
        return Err(Error::at(name.pos, "tensor names have no prime level"));
    }
    let p = pattern(name, env)?;
    if p.subscripts.contains(&SubPattern::Any) {
        let found: Vec<Name> =
            net.match_tensors(&p).into_iter().map(|t| net.tensor(t).name.clone()).collect();
        if found.is_empty() {
            return Err(Error::at(name.pos, format!("`{p}` matches no tensor")));
        }
        return Ok(found);
    }
    Ok(enumerate(&p))
}

fn enumerate(p: &NamePattern) -> Vec<Name> {
    let mut names = vec![Vec::new()];
    for s in &p.subscripts {
        let values: Vec<i64> = match *s {
            SubPattern::Exact(e) => vec![e],
            SubPattern::Range(a, b) => (a..=b).collect(),
            SubPattern::Any => unreachable!(),
        };
        names = names
            .into_iter()
            .flat_map(|prefix: Vec<i64>| {
                values.iter().map(move |&v| {
                    let mut n = prefix.clone();
                    n.push(v);
                    n
                })
            })
            .collect();
    }
    names.into_iter().map(|subs| Name::new(p.base.clone(), subs)).collect()
}

fn single_name(name: &NameAst, env: &Env) -> Result<Name> {
    let p = pattern(name, env)?;
    match p.is_exact() {
        true => Ok(enumerate(&p).remove(0)),
        false => Err(Error::at(name.pos, format!("`{p}` must name exactly one thing"))),
    }
}

/// Resolve a list of references to tensors (created when missing), with
/// their optional leg references.
fn resolve_list(net: &mut Network, list: &[EndRef], env: &Env) -> Result<Vec<(TensorId, Option<LegRef>)>> {
    let mut out = Vec::new();
    for end in list {
        for name in tensor_names(net, &end.name, env)? {
            if net.group(&name.base).is_some() && name.subscripts.is_empty() {
                let members = net.group(&name.base).unwrap().members.clone();
                out.extend(members.into_iter().map(|t| (t, end.leg.clone())));
                continue;
            }
            out.push((net.tensor_or_create(&name), end.leg.clone()));
        }
    }
    Ok(out)
}

fn leg_label(leg: &Option<LegRef>, pos: Pos) -> Result<Option<&str>> {
    match leg {
        None => Ok(None),
        Some(LegRef::Label(l)) => Ok(Some(l)),
        Some(LegRef::Position(_)) => Err(Error::at(pos, "connect legs by label, not by position")),
    }
}

/// Pair two lists: equal lengths pair element by element, and a single
/// element pairs with every element of the other list.
fn pairs<T: Clone>(a: &[T], b: &[T], pos: Pos) -> Result<Vec<(T, T)>> {
    if a.len() == b.len() {
        Ok(a.iter().cloned().zip(b.iter().cloned()).collect())
    } else if a.len() == 1 {
        Ok(b.iter().map(|y| (a[0].clone(), y.clone())).collect())
    } else if b.len() == 1 {
        Ok(a.iter().map(|x| (x.clone(), b[0].clone())).collect())
    } else {
        Err(Error::at(pos, format!("cannot pair {} tensors with {}", a.len(), b.len())))
    }
}

fn rule(net: &mut Network, selector: Selector, attrs: &[Attr]) -> Result<()> {
    net.add_rule(selector, attrs)
}

fn statement(net: &mut Network, auto: &mut Auto, stmt: &Stmt) -> Result<()> {
    let pos = stmt.pos;
    match &stmt.kind {
        StmtKind::Version(v) => {
            if !READS.contains(&v.as_str()) {
                return Err(Error::at(
                    pos,
                    format!("this reads tnv {}; the file asks for {v}", READS.join(" and ")),
                ));
            }
        }
        StmtKind::Scene(dim) => net.scene_mut().dim = *dim,
        StmtKind::Spacing(d) => {
            if *d <= 0.0 {
                return Err(Error::at(pos, "spacing must be positive"));
            }
            net.scene_mut().spacing = Some(*d);
        }
        StmtKind::Light(v) => net.scene_mut().light = Some(v.clone()),
        StmtKind::Camera { angles, attrs } => {
            let cam = net.scene_mut().camera.get_or_insert_with(Default::default);
            if angles.is_some() {
                cam.angles = *angles;
            }
            cam.attrs.extend(attrs.iter().cloned());
        }
        StmtKind::Index { name, tags, attrs, each: clause } => each(clause, |env| {
            for n in enumerate(&pattern(name, env)?) {
                let id = net.index_or_create(&n, name.prime);
                net.index_mut(id).tags.extend(tags.iter().cloned());
                let mut rest = Vec::new();
                for a in attrs {
                    if a.key == "dim" {
                        let dim = match a.value {
                            Value::Number(x) if x >= 1.0 && x == x.trunc() => x as u64,
                            _ => return Err(Error::at(name.pos, "dim must be a positive integer")),
                        };
                        net.index_mut(id).dim = Some(dim);
                    } else {
                        rest.push(a.clone());
                    }
                }
                rule(net, Selector::Index(id), &rest)?;
            }
            Ok(())
        })?,
        StmtKind::Tensor { name, slots, attrs, each: clause } => each(clause, |env| {
            let t = net.tensor_or_create(&single_name(name, env)?);
            for slot in slots {
                let index = net.index_or_create(&single_name(&slot.index, env)?, slot.index.prime);
                net.attach(t, index, slot.label.clone()).map_err(|e| e.or_at(slot.index.pos))?;
            }
            rule(net, Selector::Tensor(t), attrs)?;
            Ok(())
        })?,
        StmtKind::Declare(list) => {
            resolve_list(net, list, &Env::new())?;
        }
        StmtKind::Connect { lists, attrs, each: clause } => each(clause, |env| {
            let resolved: Vec<_> = lists.iter().map(|l| resolve_list(net, l, env)).collect::<Result<_>>()?;
            for w in resolved.windows(2) {
                for ((a, la), (b, lb)) in pairs(&w[0], &w[1], pos)? {
                    let index = connect(net, auto, a, leg_label(&la, pos)?, b, leg_label(&lb, pos)?)?;
                    rule(net, Selector::Index(index), attrs)?;
                }
            }
            Ok(())
        })?,
        StmtKind::Chain { group, list, attrs } => {
            let tensors: Vec<TensorId> =
                resolve_list(net, list, &Env::new())?.into_iter().map(|(t, _)| t).collect();
            for w in tensors.windows(2) {
                let index = connect(net, auto, w[0], None, w[1], None)?;
                rule(net, Selector::Index(index), attrs)?;
            }
            if let Some(g) = group {
                net.add_group(g, tensors.clone())?;
            }
            net.add_layout(LayoutStmt::Row { tensors })?;
        }
        StmtKind::Grid { base, rows, cols, attrs } => {
            let id = |net: &mut Network, r: usize, c: usize| {
                net.tensor_or_create(&Name::new(base.clone(), [r as i64, c as i64]))
            };
            for r in 1..=*rows {
                for c in 1..=*cols {
                    let t = id(net, r, c);
                    if c < *cols {
                        let right = id(net, r, c + 1);
                        connect(net, auto, t, None, right, None)?;
                    }
                    if r < *rows {
                        let below = id(net, r + 1, c);
                        connect(net, auto, t, None, below, None)?;
                    }
                    rule(net, Selector::Tensor(t), attrs)?;
                }
            }
            net.add_layout(LayoutStmt::Grid { base: base.clone(), rows: *rows, cols: *cols })?;
        }
        StmtKind::Legs { targets, legs } => {
            for (t, _) in resolve_list(net, targets, &Env::new())? {
                for spec in legs {
                    let slot = open_leg(net, auto, t, spec.label.clone())?;
                    if let Some(dir) = &spec.dir {
                        rule(net, Selector::Slot(t, slot), &[Attr::new("leg-dir", dir.to_value())])?;
                    }
                }
            }
        }
        StmtKind::Group { name, members } => {
            let members = resolve_list(net, members, &Env::new())?.into_iter().map(|(t, _)| t).collect();
            net.add_group(name, members)?;
        }
        StmtKind::At { target, pos: p } => {
            let tensor = net.tensor_or_create(&single_name(target, &Env::new())?);
            net.add_layout(LayoutStmt::At { tensor, pos: p.clone() })?;
        }
        StmtKind::Relative { target, relation, anchor, distance } => {
            let tensor = net.tensor_or_create(&single_name(target, &Env::new())?);
            let anchor = net.tensor_or_create(&single_name(anchor, &Env::new())?);
            net.add_layout(LayoutStmt::Relative {
                tensor,
                relation: *relation,
                anchor,
                distance: *distance,
            })?;
        }
        StmtKind::Stack(groups) => {
            net.add_layout(LayoutStmt::Stack { groups: groups.clone() })?;
        }
        StmtKind::Tree { root, dir } => {
            let root = net.tensor_or_create(&single_name(root, &Env::new())?);
            net.add_layout(LayoutStmt::Tree { root, direction: dir.clone() })?;
        }
        StmtKind::Style { selector, attrs } => {
            let env = Env::new();
            let selector = match selector {
                SelectorAst::Tensors => Selector::Tensors,
                SelectorAst::Bonds => Selector::Bonds,
                SelectorAst::Legs => Selector::Legs,
                SelectorAst::OpenLegs => Selector::OpenLegs,
                SelectorAst::Tag(t) => Selector::Tag(t.clone()),
                SelectorAst::Name(n) => Selector::Name(pattern(n, &env)?),
                SelectorAst::LegOf(n, leg) => Selector::LegOf(
                    pattern(n, &env)?,
                    match leg {
                        LegRef::Label(l) => LegKey::Label(l.clone()),
                        LegRef::Position(k) => LegKey::Position(*k),
                    },
                ),
            };
            rule(net, selector, attrs)?;
        }
    }
    Ok(())
}
