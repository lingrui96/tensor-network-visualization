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

/// A coordinate: arithmetic on numbers and `for` variables.
#[derive(Clone, Debug, PartialEq)]
pub enum Coord {
    Num(f64),
    Var(String),
    Neg(Box<Coord>),
    Op(char, Box<Coord>, Box<Coord>),
}

impl Coord {
    pub fn eval(&self, env: &std::collections::HashMap<String, i64>) -> std::result::Result<f64, String> {
        Ok(match self {
            Coord::Num(x) => *x,
            Coord::Var(v) => {
                *env.get(v).ok_or_else(|| format!("unknown variable `{v}` in a coordinate"))? as f64
            }
            Coord::Neg(a) => -a.eval(env)?,
            Coord::Op(op, a, b) => {
                let (a, b) = (a.eval(env)?, b.eval(env)?);
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    _ => a / b,
                }
            }
        })
    }
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
    Planes,
    /// `g.-`: the bonds within a group.
    GroupBonds(String),
    Tag(String),
    Name(NameAst),
    LegOf(NameAst, LegRef),
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Version(String),
    Scene(Dim),
    Spacing(f64),
    Light {
        v: Vec<f64>,
        world: bool,
    },
    View {
        x: Vec<f64>,
        y: Vec<f64>,
    },
    /// `plane P under g [...]` or `plane P at (x, y, z) [...]`.
    Plane {
        name: String,
        under: Option<String>,
        at: Option<Vec<f64>>,
        attrs: Vec<Attr>,
    },
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
        each: Option<ForClause>,
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
        pos: Vec<Coord>,
        each: Option<ForClause>,
    },
    Relative {
        target: NameAst,
        relation: Relation,
        anchor: NameAst,
        distance: Option<f64>,
        each: Option<ForClause>,
    },
    /// The statements of a function call, and the group its new tensors
    /// form, if named.
    Block {
        group: Option<String>,
        stmts: Vec<Stmt>,
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
