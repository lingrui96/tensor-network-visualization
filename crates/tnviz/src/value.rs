//! Attribute lists: `[key=value, flag, ...]`.

use std::fmt;

use crate::error::{Error, Pos, Result};

/// Units a length may carry.  Unitless numbers are layout units.
pub const UNITS: [&str; 7] = ["mm", "cm", "pt", "bp", "in", "em", "px"];

/// The value of one attribute.  Values are kept as written; their meaning is
/// decided by whoever reads the attribute.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A key without a value, such as `tube` in `[tube]`.
    Flag,
    Number(f64),
    Length(f64, String),
    Str(String),
    /// Inline math, dollar signs included, such as `$\chi$`.
    Math(String),
    /// Any other bare word, such as `blue!64!black` or `down-left`.
    Word(String),
    /// One or more coordinates, such as `(2, 1), (3, 1)`.
    Points(Vec<Vec<f64>>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attr {
    pub key: String,
    pub value: Value,
}

impl Attr {
    pub fn new(key: impl Into<String>, value: Value) -> Self {
        Attr { key: key.into(), value }
    }
}

/// Merge attribute lists in order: a later value for the same key replaces
/// an earlier one, in its original position.
pub fn merge_attrs<'a>(lists: impl IntoIterator<Item = &'a [Attr]>) -> Vec<Attr> {
    let mut out: Vec<Attr> = Vec::new();
    for list in lists {
        for attr in list {
            match out.iter_mut().find(|a| a.key == attr.key) {
                Some(existing) => existing.value = attr.value.clone(),
                None => out.push(attr.clone()),
            }
        }
    }
    out
}

pub(crate) fn format_number(x: f64) -> String {
    if x == x.trunc() && x.abs() < 1e15 { format!("{}", x as i64) } else { format!("{x}") }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Flag => Ok(()),
            Value::Number(x) => f.write_str(&format_number(*x)),
            Value::Length(x, unit) => write!(f, "{}{unit}", format_number(*x)),
            Value::Str(s) if s.contains('"') => write!(f, "#\"{s}\"#"),
            Value::Str(s) => write!(f, "\"{s}\""),
            Value::Math(s) | Value::Word(s) => f.write_str(s),
            Value::Points(points) => {
                for (k, p) in points.iter().enumerate() {
                    if k > 0 {
                        f.write_str(", ")?;
                    }
                    write_point(f, p)?;
                }
                Ok(())
            }
        }
    }
}

pub(crate) fn write_point(f: &mut impl fmt::Write, p: &[f64]) -> fmt::Result {
    f.write_str("(")?;
    for (k, x) in p.iter().enumerate() {
        if k > 0 {
            f.write_str(", ")?;
        }
        f.write_str(&format_number(*x))?;
    }
    f.write_str(")")
}

impl fmt::Display for Attr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            Value::Flag => f.write_str(&self.key),
            _ => write!(f, "{}={}", self.key, self.value),
        }
    }
}

/// Write `[a=1, b]`.
pub(crate) fn write_attrs(f: &mut impl fmt::Write, attrs: &[Attr]) -> fmt::Result {
    f.write_str("[")?;
    for (k, a) in attrs.iter().enumerate() {
        if k > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{a}")?;
    }
    f.write_str("]")
}

/// Parse the text between the brackets of an attribute list.  `pos` is the
/// position of the opening bracket, for error messages.
/// Replace every `@name` outside strings and math by the value of the
/// variable `name`.
pub fn substitute(raw: &str, vars: &[(String, String)]) -> std::result::Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let (mut quote, mut math) = (false, false);
    let mut it = raw.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '"' if !math => quote = !quote,
            '$' if !quote => math = !math,
            '@' if !quote && !math => {
                let start = i + 1;
                let mut end = start;
                while let Some(&(j, d)) = it.peek() {
                    if d.is_ascii_alphanumeric() || d == '_' {
                        end = j + d.len_utf8();
                        it.next();
                    } else {
                        break;
                    }
                }
                let name = &raw[start..end];
                if name.is_empty() {
                    return Err("`@` must be followed by a variable name".into());
                }
                let (_, value) = vars.iter().find(|(n, _)| n == name).ok_or_else(|| {
                    format!("unknown variable `@{name}`; declare it first with `let {name} = …`")
                })?;
                out.push_str(value);
                continue;
            }
            _ => {}
        }
        out.push(c);
    }
    Ok(out)
}

pub fn parse_attrs(raw: &str, pos: Pos) -> Result<Vec<Attr>> {
    let mut attrs: Vec<Attr> = Vec::new();
    for item in split_top_level(raw).map_err(|m| Error::at(pos, m))? {
        let item = item.trim();
        if item.is_empty() {
            return Err(Error::at(pos, "empty attribute"));
        }
        // `via=(2, 1), (3, 1)`: a bare coordinate continues the previous list.
        if item.starts_with('(') {
            let point = parse_point(item).map_err(|m| Error::at(pos, m))?;
            match attrs.last_mut() {
                Some(Attr { value: Value::Points(points), .. }) => points.push(point),
                _ => return Err(Error::at(pos, format!("`{item}` has no key"))),
            }
            continue;
        }
        let (key, value) = match find_top_level(item, '=') {
            Some(i) => {
                let value = parse_value(item[i + 1..].trim()).map_err(|m| Error::at(pos, m))?;
                (item[..i].trim(), value)
            }
            None => (item, Value::Flag),
        };
        if !is_key(key) {
            return Err(Error::at(pos, format!("`{key}` is not a valid attribute name")));
        }
        attrs.push(Attr::new(key, value));
    }
    Ok(attrs)
}

fn is_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Split at commas outside parentheses, braces, quotes, and math.
fn split_top_level(raw: &str) -> std::result::Result<Vec<&str>, String> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut scan = Scanner::default();
    for (i, c) in raw.char_indices() {
        if scan.is_top_level() && c == ',' {
            items.push(&raw[start..i]);
            start = i + 1;
            continue;
        }
        scan.step(raw, i, c)?;
    }
    scan.finish()?;
    items.push(&raw[start..]);
    Ok(items)
}

fn find_top_level(item: &str, target: char) -> Option<usize> {
    let mut scan = Scanner::default();
    for (i, c) in item.char_indices() {
        if scan.is_top_level() && c == target {
            return Some(i);
        }
        scan.step(item, i, c).ok()?;
    }
    None
}

/// Tracks nesting in attribute text: (), {}, "strings", #"raw strings"#,
/// and $math$.
#[derive(Default)]
struct Scanner {
    depth: i32,
    quote: bool,
    raw: bool,
    math: bool,
}

impl Scanner {
    fn is_top_level(&self) -> bool {
        self.depth == 0 && !self.quote && !self.raw && !self.math
    }

    fn step(&mut self, s: &str, i: usize, c: char) -> std::result::Result<(), String> {
        if self.raw {
            if c == '"' && s[i + 1..].starts_with('#') {
                self.raw = false;
            }
            return Ok(());
        }
        if self.quote {
            if c == '"' && !s[..i].ends_with('\\') {
                self.quote = false;
            }
            return Ok(());
        }
        match c {
            '#' if s[i + 1..].starts_with('"') => self.raw = true,
            '"' if !s[..i].ends_with('#') => self.quote = true,
            '$' => self.math = !self.math,
            '(' | '{' => self.depth += 1,
            ')' | '}' => {
                self.depth -= 1;
                if self.depth < 0 {
                    return Err(format!("unbalanced `{c}`"));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&self) -> std::result::Result<(), String> {
        if self.quote || self.raw {
            Err("unterminated string".into())
        } else if self.math {
            Err("unterminated math".into())
        } else if self.depth != 0 {
            Err("unbalanced parentheses".into())
        } else {
            Ok(())
        }
    }
}

fn parse_value(s: &str) -> std::result::Result<Value, String> {
    if s.is_empty() {
        return Err("missing value".into());
    }
    if let Some(inner) = s.strip_prefix("#\"").and_then(|r| r.strip_suffix("\"#")) {
        return Ok(Value::Str(inner.to_string()));
    }
    if let Some(inner) = s.strip_prefix('"') {
        let inner = inner.strip_suffix('"').ok_or("unterminated string")?;
        return Ok(Value::Str(inner.replace("\\\"", "\"")));
    }
    if s.starts_with('$') {
        if s.len() < 2 || !s.ends_with('$') {
            return Err(format!("`{s}` is not closed by `$`"));
        }
        return Ok(Value::Math(s.to_string()));
    }
    if s.starts_with('(') {
        return Ok(Value::Points(vec![parse_point(s)?]));
    }
    if let Some(v) = parse_number(s) {
        return Ok(v);
    }
    if s.chars().any(char::is_whitespace) {
        return Err(format!("`{s}` contains spaces; quote it"));
    }
    Ok(Value::Word(s.to_string()))
}

/// A number with an optional unit, such as `2`, `-.5`, `2mm`, or `.08em`.
pub(crate) fn parse_number(s: &str) -> Option<Value> {
    let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '+') {
        return None;
    }
    let x: f64 = num.parse().ok()?;
    if unit.is_empty() {
        Some(Value::Number(x))
    } else if UNITS.contains(&unit) {
        Some(Value::Length(x, unit.to_string()))
    } else {
        None
    }
}

fn parse_point(s: &str) -> std::result::Result<Vec<f64>, String> {
    let inner = s
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .ok_or_else(|| format!("`{s}` is not a coordinate"))?;
    let coords: Vec<f64> = inner
        .split(',')
        .map(|c| c.trim().parse::<f64>().map_err(|_| format!("`{}` is not a number", c.trim())))
        .collect::<std::result::Result<_, _>>()?;
    if coords.len() == 2 || coords.len() == 3 {
        Ok(coords)
    } else {
        Err(format!("`{s}` must have 2 or 3 coordinates"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<Attr> {
        parse_attrs(s, Pos::default()).unwrap()
    }

    #[test]
    fn values() {
        let a = p(
            r#"tube, width=3.2mm, color=blue!64!black, label=$T_{1,2}$, via=(2, 1), (3, -1.5), n=.5, s="a b""#,
        );
        assert_eq!(a[0], Attr::new("tube", Value::Flag));
        assert_eq!(a[1].value, Value::Length(3.2, "mm".into()));
        assert_eq!(a[2].value, Value::Word("blue!64!black".into()));
        assert_eq!(a[3].value, Value::Math("$T_{1,2}$".into()));
        assert_eq!(a[4].value, Value::Points(vec![vec![2.0, 1.0], vec![3.0, -1.5]]));
        assert_eq!(a[5].value, Value::Number(0.5));
        assert_eq!(a[6].value, Value::Str("a b".into()));
    }

    #[test]
    fn raw_strings_and_errors() {
        assert_eq!(p(r##"label=#"say "hi""#"##)[0].value, Value::Str("say \"hi\"".into()));
        assert!(parse_attrs("label=$\\chi", Pos::default()).is_err());
        assert!(parse_attrs("a=1,,b", Pos::default()).is_err());
        assert!(parse_attrs("(1, 2)", Pos::default()).is_err());
    }

    #[test]
    fn merge() {
        let a = p("color=red, tube");
        let b = p("color=blue, width=2");
        let m = merge_attrs([a.as_slice(), b.as_slice()]);
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].value, Value::Word("blue".into()));
    }
}
