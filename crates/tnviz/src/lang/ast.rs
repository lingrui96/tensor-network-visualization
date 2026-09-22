//! Syntax tree of tnv source, before names are resolved.

use crate::error::Pos;
use crate::model::{Dim, Direction, Relation};
use crate::value::Attr;

/// A subscript expression: an integer, or a loop variable plus an offset.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Int(i64),
    Var(String, i64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Sub {
    Expr(Expr),
    Range(Expr, Expr),
    Any,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NameAst {
    pub base: String,
    pub subs: Vec<Sub>,
    pub prime: u32,
    pub pos: Pos,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LegRef {
    Label(String),
    Position(usize),
}

/// `A`, `A[*]`, or `A.r`.
#[derive(Clone, Debug, PartialEq)]
pub struct EndRef {
    pub name: NameAst,
    pub leg: Option<LegRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlotAst {
    pub label: Option<String>,
    pub index: NameAst,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LegSpec {
    pub label: Option<String>,
    pub dir: Option<Direction>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ForClause {
    pub var: String,
    pub from: i64,
    pub to: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SelectorAst {
    Tensors,
    Bonds,
    Legs,
    OpenLegs,
    Tag(String),
    Name(NameAst),
    LegOf(NameAst, LegRef),
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Version(String),
    Scene(Dim),
    Spacing(f64),
    Light(Vec<f64>),
    Camera {
        angles: Option<(f64, f64)>,
        attrs: Vec<Attr>,
    },
    Index {
        name: NameAst,
        tags: Vec<String>,
        attrs: Vec<Attr>,
        each: Option<ForClause>,
    },
    Tensor {
        name: NameAst,
        slots: Vec<SlotAst>,
        attrs: Vec<Attr>,
        each: Option<ForClause>,
    },
    /// Tensors named on their own line.
    Declare(Vec<EndRef>),
    Connect {
        lists: Vec<Vec<EndRef>>,
        attrs: Vec<Attr>,
        each: Option<ForClause>,
    },
    Chain {
        group: Option<String>,
        list: Vec<EndRef>,
        attrs: Vec<Attr>,
    },
    Grid {
        base: String,
        rows: usize,
        cols: usize,
        attrs: Vec<Attr>,
    },
    Legs {
        targets: Vec<EndRef>,
        legs: Vec<LegSpec>,
    },
    Group {
        name: String,
        members: Vec<EndRef>,
    },
    At {
        target: NameAst,
        pos: Vec<f64>,
    },
    Relative {
        target: NameAst,
        relation: Relation,
        anchor: NameAst,
        distance: Option<f64>,
    },
    Stack(Vec<String>),
    Tree {
        root: NameAst,
        dir: Direction,
    },
    Style {
        selector: SelectorAst,
        attrs: Vec<Attr>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub pos: Pos,
    pub kind: StmtKind,
}
