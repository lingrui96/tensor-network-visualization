//! Parse tokens into statements.

use super::ast::*;
use super::lexer::{Tok, Token, lex};
use crate::error::{Error, Pos, Result};
use crate::model::{Dim, Direction, Relation};
use crate::value::{Attr, parse_attrs, substitute};

pub fn parse(src: &str) -> Result<Vec<Stmt>> {
    Parser { toks: lex(src)?, i: 0, vars: Vec::new() }.file()
}

struct Parser {
    toks: Vec<Token>,
    i: usize,
    /// `let` variables in order of declaration, with their values expanded.
    vars: Vec<(String, String)>,
}

impl Parser {
    // ---- Token access ----------------------------------------------------

    fn peek(&self) -> &Tok {
        &self.toks[self.i].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        &self.toks[(self.i + n).min(self.toks.len() - 1)].tok
    }

    fn pos(&self) -> Pos {
        self.toks[self.i].pos
    }

    fn attached(&self) -> bool {
        self.toks[self.i].attached
    }

    fn advance(&mut self) -> Tok {
        let tok = self.peek().clone();
        if tok != Tok::Eof {
            self.i += 1;
        }
        tok
    }

    fn is_sym(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::Sym(x) if *x == s)
    }

    fn eat_sym(&mut self, s: &str) -> bool {
        let hit = self.is_sym(s);
        if hit {
            self.advance();
        }
        hit
    }

    fn expect_sym(&mut self, s: &str) -> Result<()> {
        if self.eat_sym(s) { Ok(()) } else { self.fail(&format!("expected `{s}`")) }
    }

    fn is_ident(&self, word: &str) -> bool {
        matches!(self.peek(), Tok::Ident(w) if w == word)
    }

    fn eat_ident(&mut self, word: &str) -> bool {
        let hit = self.is_ident(word);
        if hit {
            self.advance();
        }
        hit
    }

    fn ident(&mut self, what: &str) -> Result<String> {
        match self.peek().clone() {
            Tok::Ident(w) => {
                self.advance();
                Ok(w)
            }
            _ => self.fail(&format!("expected {what}")),
        }
    }

    fn at_end(&self) -> bool {
        matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::Sym(";"))
    }

    fn describe(tok: &Tok) -> String {
        match tok {
            Tok::Ident(w) => format!("`{w}`"),
            Tok::Int(n) => format!("`{n}`"),
            Tok::Num(x) => format!("`{x}`"),
            Tok::Length(x, u) => format!("`{x}{u}`"),
            Tok::Dims(r, c) => format!("`{r}x{c}`"),
            Tok::Attrs(_) => "an attribute list".into(),
            Tok::Raw(_) => "a value".into(),
            Tok::Sym(s) => format!("`{s}`"),
            Tok::Newline => "the end of the line".into(),
            Tok::Eof => "the end of the file".into(),
        }
    }

    fn fail<T>(&self, msg: &str) -> Result<T> {
        Err(Error::at(self.pos(), format!("{msg}, found {}", Self::describe(self.peek()))))
    }

    // ---- File and statements ---------------------------------------------

    fn file(mut self) -> Result<Vec<Stmt>> {
        let mut stmts = Vec::new();
        loop {
            while matches!(self.peek(), Tok::Newline | Tok::Sym(";")) {
                self.advance();
            }
            if *self.peek() == Tok::Eof {
                return Ok(stmts);
            }
            if self.is_ident("let") {
                self.let_statement()?;
                if !self.at_end() {
                    return self.fail("expected the end of the statement");
                }
                continue;
            }
            let stmt = self.statement()?;
            // `3d, camera 35 25`: a comma may follow a scene statement.
            let chained = matches!(stmt.kind, StmtKind::Scene(_)) && self.eat_sym(",");
            stmts.push(stmt);
            if !chained && !self.at_end() {
                return self.fail("expected the end of the statement");
            }
        }
    }

    fn statement(&mut self) -> Result<Stmt> {
        let pos = self.pos();
        let kind = match self.peek().clone() {
            Tok::Ident(w) => match w.as_str() {
                "tnv" if matches!(self.peek_at(1), Tok::Num(_) | Tok::Int(_)) => {
                    self.advance();
                    let version = match self.advance() {
                        Tok::Num(x) => format!("{x}"),
                        Tok::Int(n) => format!("{n}"),
                        _ => unreachable!(),
                    };
                    StmtKind::Version(version)
                }
                "2d" | "3d" => {
                    self.advance();
                    StmtKind::Scene(if w == "2d" { Dim::Two } else { Dim::Three })
                }
                "spacing" => {
                    self.advance();
                    StmtKind::Spacing(self.number()?)
                }
                "light" => {
                    self.advance();
                    if self.is_sym("(") {
                        StmtKind::Light(self.point()?)
                    } else {
                        StmtKind::Light(vec![self.number()?])
                    }
                }
                "camera" => {
                    self.advance();
                    let angles = if matches!(self.peek(), Tok::Int(_) | Tok::Num(_) | Tok::Sym("-")) {
                        Some((self.number()?, self.number()?))
                    } else {
                        None
                    };
                    StmtKind::Camera { angles, attrs: self.attrs_opt()? }
                }
                "index" => {
                    self.advance();
                    let name = self.name()?;
                    let mut tags = Vec::new();
                    if self.eat_sym(":") {
                        tags.push(self.ident("a tag")?);
                        while self.eat_sym(",") {
                            tags.push(self.ident("a tag")?);
                        }
                    }
                    let attrs = self.attrs_opt()?;
                    StmtKind::Index { name, tags, attrs, each: self.for_opt()? }
                }
                "tensor" => {
                    self.advance();
                    let name = self.name()?;
                    self.expect_sym("(")?;
                    let mut slots = Vec::new();
                    if !self.is_sym(")") {
                        loop {
                            let label = if matches!(self.peek(), Tok::Ident(_))
                                && matches!(self.peek_at(1), Tok::Sym("="))
                            {
                                let l = self.ident("a leg label")?;
                                self.advance();
                                Some(l)
                            } else {
                                None
                            };
                            slots.push(SlotAst { label, index: self.name()? });
                            if !self.eat_sym(",") {
                                break;
                            }
                        }
                    }
                    self.expect_sym(")")?;
                    let attrs = self.attrs_opt()?;
                    StmtKind::Tensor { name, slots, attrs, each: self.for_opt()? }
                }
                "chain" => {
                    self.advance();
                    self.chain(None)?
                }
                "grid" => {
                    self.advance();
                    let base = self.ident("a tensor name")?;
                    let Tok::Dims(rows, cols) = *self.peek() else {
                        return self.fail("expected a size such as 3x3");
                    };
                    self.advance();
                    StmtKind::Grid { base, rows, cols, attrs: self.attrs_opt()? }
                }
                "stack" => {
                    self.advance();
                    let mut groups = vec![self.ident("a group name")?];
                    while self.eat_sym(",") {
                        groups.push(self.ident("a group name")?);
                    }
                    StmtKind::Stack(groups)
                }
                "tree" => {
                    self.advance();
                    let root = self.name()?;
                    let Some(dir) = self.direction_opt()? else {
                        return self.fail("expected a direction such as `down`");
                    };
                    StmtKind::Tree { root, dir }
                }
                "leg" if matches!(self.peek_at(1), Tok::Attrs(_) | Tok::Sym(".")) => {
                    self.advance();
                    let selector = if self.eat_sym(".") {
                        if !self.eat_ident("open") {
                            return self.fail("expected `open`");
                        }
                        SelectorAst::OpenLegs
                    } else {
                        SelectorAst::Legs
                    };
                    StmtKind::Style { selector, attrs: self.attrs()? }
                }
                "tag" if matches!(self.peek_at(1), Tok::Sym(":")) => {
                    self.advance();
                    self.advance();
                    let tag = self.ident("a tag")?;
                    StmtKind::Style { selector: SelectorAst::Tag(tag), attrs: self.attrs()? }
                }
                _ => self.name_statement()?,
            },
            Tok::Sym("*") => {
                self.advance();
                StmtKind::Style { selector: SelectorAst::Tensors, attrs: self.attrs()? }
            }
            Tok::Sym("-") => {
                self.advance();
                StmtKind::Style { selector: SelectorAst::Bonds, attrs: self.attrs()? }
            }
            _ => return self.fail("expected a statement"),
        };
        Ok(Stmt { pos, kind })
    }

    /// Statements that start with names: connections, legs, groups,
    /// placement, and styles.
    fn name_statement(&mut self) -> Result<StmtKind> {
        let first = self.end_list()?;
        if self.eat_sym(":") {
            if self.eat_ident("leg") || self.eat_ident("legs") {
                no_legs(&first)?;
                return Ok(StmtKind::Legs { targets: first, legs: self.leg_specs()? });
            }
            let group = self.group_name(&first)?;
            if self.eat_ident("chain") {
                return self.chain(Some(group));
            }
            let members = self.end_list()?;
            no_legs(&members)?;
            return Ok(StmtKind::Group { name: group, members });
        }
        if self.eat_ident("at") {
            let target = self.single_name(first)?;
            return Ok(StmtKind::At { target, pos: self.point()? });
        }
        let relation = if self.is_ident("right") || self.is_ident("left") {
            let right = self.is_ident("right");
            self.advance();
            if !self.eat_ident("of") {
                return self.fail("expected `of`");
            }
            Some(if right { Relation::RightOf } else { Relation::LeftOf })
        } else if self.eat_ident("above") {
            Some(Relation::Above)
        } else if self.eat_ident("below") {
            Some(Relation::Below)
        } else {
            None
        };
        if let Some(relation) = relation {
            let target = self.single_name(first)?;
            let anchor = self.name()?;
            let distance = if self.eat_sym(",") { Some(self.number()?) } else { None };
            return Ok(StmtKind::Relative { target, relation, anchor, distance });
        }
        if self.is_sym("-") {
            let mut lists = vec![first];
            while self.eat_sym("-") {
                lists.push(self.end_list()?);
            }
            let attrs = self.attrs_opt()?;
            return Ok(StmtKind::Connect { lists, attrs, each: self.for_opt()? });
        }
        if matches!(self.peek(), Tok::Attrs(_)) {
            let [end]: [EndRef; 1] = first.try_into().map_err(|_| {
                Error::at(self.pos(), "a style applies to one selector; use a pattern such as A[*]")
            })?;
            let selector = match end.leg {
                Some(leg) => SelectorAst::LegOf(end.name, leg),
                None => SelectorAst::Name(end.name),
            };
            return Ok(StmtKind::Style { selector, attrs: self.attrs()? });
        }
        if self.at_end() {
            no_legs(&first)?;
            return Ok(StmtKind::Declare(first));
        }
        self.fail("expected `-`, `:`, `at`, a relation, or an attribute list")
    }

    fn group_name(&self, list: &[EndRef]) -> Result<String> {
        match list {
            [EndRef { name, leg: None }] if name.subs.is_empty() && name.prime == 0 => Ok(name.base.clone()),
            _ => Err(Error::at(list[0].name.pos, "a group name must be a plain identifier")),
        }
    }

    fn single_name(&self, list: Vec<EndRef>) -> Result<NameAst> {
        let pos = list[0].name.pos;
        match <[EndRef; 1]>::try_from(list) {
            Ok([EndRef { name, leg: None }]) => Ok(name),
            _ => Err(Error::at(pos, "expected a single tensor name")),
        }
    }

    fn chain(&mut self, group: Option<String>) -> Result<StmtKind> {
        let list = self.end_list()?;
        Ok(StmtKind::Chain { group, list, attrs: self.attrs_opt()? })
    }

    // ---- Pieces ----------------------------------------------------------

    fn attrs(&mut self) -> Result<Vec<Attr>> {
        match self.attrs_maybe()? {
            Some(a) => Ok(a),
            None => self.fail("expected an attribute list such as [color=red]"),
        }
    }

    fn attrs_opt(&mut self) -> Result<Vec<Attr>> {
        Ok(self.attrs_maybe()?.unwrap_or_default())
    }

    /// `let name = value`: a global variable, used as `@name` in attribute
    /// values.  Its value is text, typed where it is used.
    fn let_statement(&mut self) -> Result<()> {
        self.advance();
        let pos = self.pos();
        let name = self.ident("a variable name")?;
        self.expect_sym("=")?;
        let raw = match self.advance() {
            Tok::Raw(raw) => raw,
            _ => return Err(Error::at(pos, "`let` needs a value after `=`")),
        };
        if self.vars.iter().any(|(n, _)| *n == name) {
            return Err(Error::at(pos, format!("`@{name}` is already declared")));
        }
        let value = substitute(&raw, &self.vars).map_err(|m| Error::at(pos, m))?;
        self.vars.push((name, value));
        Ok(())
    }

    fn attrs_maybe(&mut self) -> Result<Option<Vec<Attr>>> {
        if let Tok::Attrs(raw) = self.peek().clone() {
            let pos = self.pos();
            self.advance();
            let raw = substitute(&raw, &self.vars).map_err(|m| Error::at(pos, m))?;
            Ok(Some(parse_attrs(&raw, pos)?))
        } else {
            Ok(None)
        }
    }

    fn for_opt(&mut self) -> Result<Option<ForClause>> {
        if !self.eat_ident("for") {
            return Ok(None);
        }
        let var = self.ident("a loop variable")?;
        if !self.eat_ident("in") {
            return self.fail("expected `in`");
        }
        let from = self.int()?;
        self.expect_sym("..")?;
        let to = self.int()?;
        Ok(Some(ForClause { var, from, to }))
    }

    fn int(&mut self) -> Result<i64> {
        let neg = self.eat_sym("-");
        match self.peek().clone() {
            Tok::Int(n) => {
                self.advance();
                Ok(if neg { -n } else { n })
            }
            _ => self.fail("expected an integer"),
        }
    }

    fn number(&mut self) -> Result<f64> {
        let neg = self.eat_sym("-");
        let x = match self.peek().clone() {
            Tok::Int(n) => n as f64,
            Tok::Num(x) => x,
            _ => return self.fail("expected a number"),
        };
        self.advance();
        Ok(if neg { -x } else { x })
    }

    fn point(&mut self) -> Result<Vec<f64>> {
        self.expect_sym("(")?;
        let mut p = vec![self.number()?];
        while self.eat_sym(",") {
            p.push(self.number()?);
        }
        self.expect_sym(")")?;
        if p.len() == 2 || p.len() == 3 { Ok(p) } else { self.fail("a coordinate has 2 or 3 components") }
    }

    fn name(&mut self) -> Result<NameAst> {
        let pos = self.pos();
        let base = self.ident("a name")?;
        let mut subs = Vec::new();
        if self.is_sym("[") && self.attached() {
            self.advance();
            loop {
                subs.push(if self.eat_sym("*") {
                    Sub::Any
                } else {
                    let a = self.expr()?;
                    if self.eat_sym("..") { Sub::Range(a, self.expr()?) } else { Sub::Expr(a) }
                });
                if !self.eat_sym(",") {
                    break;
                }
            }
            self.expect_sym("]")?;
        }
        let mut prime = 0;
        while self.is_sym("'") && self.attached() {
            self.advance();
            prime += 1;
        }
        Ok(NameAst { base, subs, prime, pos })
    }

    fn expr(&mut self) -> Result<Expr> {
        if let Tok::Ident(var) = self.peek().clone() {
            self.advance();
            let offset = if self.eat_sym("+") {
                self.int()?
            } else if self.is_sym("-") && matches!(self.peek_at(1), Tok::Int(_)) {
                self.advance();
                -self.int()?
            } else {
                0
            };
            return Ok(Expr::Var(var, offset));
        }
        Ok(Expr::Int(self.int()?))
    }

    fn end_ref(&mut self) -> Result<EndRef> {
        let name = self.name()?;
        let leg = if self.is_sym(".") && self.attached() {
            self.advance();
            if self.eat_sym("#") {
                let n = self.int()?;
                if n < 1 {
                    return self.fail("leg positions start at 1");
                }
                Some(LegRef::Position(n as usize))
            } else {
                Some(LegRef::Label(self.ident("a leg label")?))
            }
        } else {
            None
        };
        Ok(EndRef { name, leg })
    }

    fn end_list(&mut self) -> Result<Vec<EndRef>> {
        let mut list = vec![self.end_ref()?];
        while self.eat_sym(",") {
            list.push(self.end_ref()?);
        }
        Ok(list)
    }

    fn leg_specs(&mut self) -> Result<Vec<LegSpec>> {
        let mut specs = Vec::new();
        if self.at_end() {
            specs.push(LegSpec { label: None, dir: None });
            return Ok(specs);
        }
        loop {
            let label = match self.peek().clone() {
                Tok::Ident(w) if !Direction::WORDS.contains(&w.as_str()) => {
                    self.advance();
                    Some(w)
                }
                _ => None,
            };
            let dir = self.direction_opt()?;
            specs.push(LegSpec { label, dir });
            if !self.eat_sym(",") {
                return Ok(specs);
            }
        }
    }

    /// A direction: `down`, `down-left`, `+z`, `-45`, `30`, or `(0, 0, 1)`.
    fn direction_opt(&mut self) -> Result<Option<Direction>> {
        match self.peek().clone() {
            Tok::Sym(sign @ ("+" | "-")) => {
                if let Tok::Ident(axis) = self.peek_at(1).clone()
                    && matches!(axis.as_str(), "x" | "y" | "z")
                    && self.toks[self.i + 1].attached
                {
                    self.advance();
                    self.advance();
                    return Ok(Some(Direction::Axis(format!("{sign}{axis}"))));
                }
                Ok(Some(Direction::Angle(self.number()?)))
            }
            Tok::Int(_) | Tok::Num(_) => Ok(Some(Direction::Angle(self.number()?))),
            Tok::Sym("(") => Ok(Some(Direction::Vector(self.point()?))),
            Tok::Ident(w) if Direction::WORDS.contains(&w.as_str()) => {
                self.advance();
                let mut word = w;
                if self.is_sym("-")
                    && self.attached()
                    && let Tok::Ident(second) = self.peek_at(1).clone()
                {
                    self.advance();
                    self.advance();
                    word = format!("{word}-{second}");
                }
                if !Direction::WORDS.contains(&word.as_str()) {
                    return Err(Error::at(self.pos(), format!("`{word}` is not a direction")));
                }
                Ok(Some(Direction::Word(word)))
            }
            _ => Ok(None),
        }
    }
}

fn no_legs(list: &[EndRef]) -> Result<()> {
    match list.iter().find(|e| e.leg.is_some()) {
        Some(e) => Err(Error::at(e.name.pos, "a leg is not allowed here")),
        None => Ok(()),
    }
}
