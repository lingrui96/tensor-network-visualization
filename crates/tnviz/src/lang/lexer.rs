//! Tokens of tnv source.
//!
//! Brackets have two roles, told apart by spacing: `A[3]` (no space before
//! the bracket) is a subscript, while `A [color=red]` is an attribute list.
//! An attribute list is returned as one raw token and parsed separately.

use crate::error::{Error, Pos, Result};
use crate::value::UNITS;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Num(f64),
    Length(f64, String),
    /// `3x3`
    Dims(usize, usize),
    /// The raw text of an attribute list, without its brackets.
    Attrs(String),
    /// The raw value of a `let`, up to the end of its line or a `;`.
    Raw(String),
    Sym(&'static str),
    Newline,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub pos: Pos,
    /// No whitespace between this token and the previous one.
    pub attached: bool,
}

const SYMBOLS: [&str; 15] = ["..", "-", ":", ";", ",", "(", ")", "*", ".", "'", "[", "]", "=", "+", "#"];

pub fn lex(src: &str) -> Result<Vec<Token>> {
    Lexer { chars: src.char_indices().collect(), src, k: 0, line: 1, col: 1, depth: 0 }.run()
}

struct Lexer<'a> {
    src: &'a str,
    chars: Vec<(usize, char)>,
    k: usize,
    line: usize,
    col: usize,
    /// Nesting of `(` and subscript `[`, inside which newlines are ignored.
    depth: i32,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl Lexer<'_> {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.k + ahead).map(|&(_, c)| c)
    }

    fn pos(&self) -> Pos {
        Pos { line: self.line, col: self.col }
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.k += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn offset(&self) -> usize {
        self.chars.get(self.k).map_or(self.src.len(), |&(i, _)| i)
    }

    fn run(mut self) -> Result<Vec<Token>> {
        let mut out: Vec<Token> = Vec::new();
        let mut attached = false;
        while let Some(c) = self.peek(0) {
            let pos = self.pos();
            // Whitespace and comments.
            if c == '\n' {
                self.bump();
                if self.depth == 0 {
                    out.push(Token { tok: Tok::Newline, pos, attached: false });
                }
                attached = false;
                continue;
            }
            if c.is_whitespace() {
                self.bump();
                attached = false;
                continue;
            }
            if c == '/' && self.peek(1) == Some('/') {
                while self.peek(0).is_some_and(|c| c != '\n') {
                    self.bump();
                }
                continue;
            }
            let prev_char = self.k.checked_sub(1).map(|j| self.chars[j].1);
            let tok = if c == '[' && !attached_to_name(prev_char) {
                self.attrs(pos)?
            } else if is_ident_start(c) {
                let mut s = String::new();
                while let Some(c) = self.peek(0).filter(|&c| is_ident_char(c)) {
                    s.push(c);
                    self.bump();
                }
                Tok::Ident(s)
            } else if c.is_ascii_digit()
                || (c == '.'
                    && self.peek(1).is_some_and(|d| d.is_ascii_digit())
                    && !prev_char.is_some_and(|p| is_ident_char(p) || p == ']' || p == '.'))
            {
                self.number(pos)?
            } else if let Some(sym) = SYMBOLS.iter().find(|s| self.src[self.offset()..].starts_with(**s)) {
                for _ in 0..sym.len() {
                    self.bump();
                }
                match *sym {
                    "(" | "[" => self.depth += 1,
                    ")" | "]" => self.depth -= 1,
                    _ => {}
                }
                Tok::Sym(sym)
            } else if c == '"' || c == '$' {
                return Err(Error::at(
                    pos,
                    "text and math belong in an attribute list, as in [label=$\\chi$]",
                ));
            } else {
                return Err(Error::at(pos, format!("unexpected character `{c}`")));
            };
            // `let name =`: the rest of the statement is a raw value.
            let starts_let = tok == Tok::Sym("=")
                && out.len() >= 2
                && out[out.len() - 2].tok == Tok::Ident("let".into())
                && matches!(out[out.len() - 1].tok, Tok::Ident(_))
                && out
                    .len()
                    .checked_sub(3)
                    .is_none_or(|j| matches!(out[j].tok, Tok::Newline | Tok::Sym(";")));
            out.push(Token { tok, pos, attached });
            attached = true;
            if starts_let {
                let pos = self.pos();
                out.push(Token { tok: self.raw_value(pos)?, pos, attached: false });
            }
        }
        out.push(Token { tok: Tok::Eof, pos: self.pos(), attached: false });
        Ok(out)
    }

    /// A number, possibly with a unit (`2mm`), or `3x3`, `2d`, `3d`.
    fn number(&mut self, pos: Pos) -> Result<Tok> {
        let start = self.offset();
        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        let mut is_float = false;
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|d| d.is_ascii_digit()) {
            is_float = true;
            self.bump();
            while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
        }
        let num_end = self.offset();
        while self.peek(0).is_some_and(is_ident_char) {
            self.bump();
        }
        let text = &self.src[start..self.offset()];
        let (num, suffix) = (&self.src[start..num_end], &self.src[num_end..self.offset()]);
        if suffix.is_empty() {
            return Ok(if is_float {
                Tok::Num(num.parse().map_err(|_| Error::at(pos, format!("bad number `{num}`")))?)
            } else {
                Tok::Int(num.parse().map_err(|_| Error::at(pos, format!("bad number `{num}`")))?)
            });
        }
        if UNITS.contains(&suffix) {
            let x = num.parse().map_err(|_| Error::at(pos, format!("bad number `{num}`")))?;
            return Ok(Tok::Length(x, suffix.to_string()));
        }
        if text == "2d" || text == "3d" {
            return Ok(Tok::Ident(text.to_string()));
        }
        if !is_float
            && let Some(c) = suffix.strip_prefix('x')
            && let (Ok(r), Ok(c)) = (num.parse(), c.parse())
        {
            return Ok(Tok::Dims(r, c));
        }
        Err(Error::at(pos, format!("`{text}` is not a number, a length, or a size like 3x3")))
    }

    /// A `let` value: up to a `;`, a `//` comment, or the end of the line,
    /// outside strings, math, and parentheses.
    fn raw_value(&mut self, pos: Pos) -> Result<Tok> {
        let start = self.offset();
        let (mut depth, mut quote, mut math) = (0i32, false, false);
        while let Some(c) = self.peek(0) {
            let top = depth == 0 && !quote && !math;
            if c == '\n' || (top && (c == ';' || (c == '/' && self.peek(1) == Some('/')))) {
                break;
            }
            match c {
                '"' if !math => quote = !quote,
                '$' if !quote => math = !math,
                '(' if !quote && !math => depth += 1,
                ')' if !quote && !math => depth -= 1,
                _ => {}
            }
            self.bump();
        }
        let raw = self.src[start..self.offset()].trim();
        if raw.is_empty() {
            return Err(Error::at(pos, "`let` needs a value after `=`"));
        }
        Ok(Tok::Raw(raw.to_string()))
    }

    /// The raw text of an attribute list; the lexer is at `[`.
    fn attrs(&mut self, pos: Pos) -> Result<Tok> {
        self.bump();
        let start = self.offset();
        let (mut depth, mut quote, mut math) = (0i32, false, false);
        loop {
            let Some(c) = self.peek(0) else {
                return Err(Error::at(pos, "attribute list is not closed by `]`"));
            };
            match c {
                '"' => quote = !quote,
                '$' if !quote => math = !math,
                '[' if !quote && !math => depth += 1,
                ']' if !quote && !math => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                _ => {}
            }
            self.bump();
        }
        let raw = self.src[start..self.offset()].to_string();
        self.bump();
        Ok(Tok::Attrs(raw))
    }
}

/// A `[` right after a name or a subscript starts a subscript.
fn attached_to_name(prev: Option<char>) -> bool {
    prev.is_some_and(|p| is_ident_char(p) || p == ']')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn subscripts_and_attrs() {
        use Tok::*;
        assert_eq!(
            toks("A[1..6] [color=red]"),
            vec![
                Ident("A".into()),
                Sym("["),
                Int(1),
                Sym(".."),
                Int(6),
                Sym("]"),
                Attrs("color=red".into()),
                Eof
            ]
        );
        assert_eq!(toks("- [label=$a[1]$]"), vec![Sym("-"), Attrs("label=$a[1]$".into()), Eof]);
    }

    #[test]
    fn numbers() {
        use Tok::*;
        assert_eq!(
            toks("3x3 2mm .5 3d"),
            vec![Dims(3, 3), Length(2.0, "mm".into()), Num(0.5), Ident("3d".into()), Eof]
        );
        assert!(lex("3kg").is_err());
    }

    #[test]
    fn newlines_inside_parentheses() {
        let t = toks("tensor A (a,\n b)\nB");
        assert_eq!(t.iter().filter(|t| **t == Tok::Newline).count(), 1);
    }
}
