//! The `.tnvm` file that LaTeX writes (docs/protocol.md, section 2): the
//! figures of a document and the typeset size of their labels.

use crate::error::{Error, Pos, Result};
use crate::geometry::{GeometryOptions, LabelSizes};

/// The `.tnvm` format version this crate reads.
pub const TNVM_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct TnvmFigure {
    /// The figure's name, part of its `.tikz` file name.
    pub name: String,
    /// TeX points per layout unit.
    pub unit_pt: f64,
    /// TeX points per em of the figure's surrounding font.
    pub em_pt: f64,
    /// The tnv source file, as LaTeX named it.
    pub source: String,
    /// Measured label sizes, in TeX points.
    pub labels: LabelSizes,
}

impl TnvmFigure {
    pub fn options(&self) -> GeometryOptions {
        GeometryOptions { unit_pt: self.unit_pt, em_pt: self.em_pt }
    }
}

/// Read a `.tnvm` file.
pub fn parse_tnvm(src: &str) -> Result<Vec<TnvmFigure>> {
    let mut figures: Vec<TnvmFigure> = Vec::new();
    let mut version = None;
    for (k, line) in src.lines().enumerate() {
        let pos = Pos { line: k + 1, col: 1 };
        let line = line.trim();
        if line.is_empty() || line.starts_with('%') {
            continue;
        }
        let (record, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        match (record, version) {
            ("tnvm", None) => {
                let v: u32 = rest.parse().map_err(|_| Error::at(pos, format!("bad version `{rest}`")))?;
                if v != TNVM_VERSION {
                    return Err(Error::at(
                        pos,
                        format!("this is .tnvm version {v}; tnviz reads version {TNVM_VERSION}"),
                    ));
                }
                version = Some(v);
            }
            (_, None) => return Err(Error::at(pos, "a .tnvm file starts with `tnvm <version>`")),
            ("figure", Some(_)) => {
                let figure = parse_figure(rest).map_err(|m| Error::at(pos, m))?;
                if figures.iter().any(|f| f.name == figure.name) {
                    return Err(Error::at(pos, format!("figure `{}` appears twice", figure.name)));
                }
                figures.push(figure);
            }
            ("label", Some(_)) => {
                let fields: Vec<&str> = rest.split_whitespace().collect();
                let [figure, id, w, h, d] = fields[..] else {
                    return Err(Error::at(pos, "expected `label <figure> <id> <width> <height> <depth>`"));
                };
                let number =
                    |s: &str| s.parse::<f64>().map_err(|_| Error::at(pos, format!("bad length `{s}`")));
                let (w, h, d) = (number(w)?, number(h)?, number(d)?);
                let Some(f) = figures.iter_mut().find(|f| f.name == figure) else {
                    return Err(Error::at(pos, format!("label of unknown figure `{figure}`")));
                };
                f.labels.insert(id, w, h, d);
            }
            (other, Some(_)) => return Err(Error::at(pos, format!("unknown record `{other}`"))),
        }
    }
    if version.is_none() {
        return Err(Error::new("empty .tnvm file"));
    }
    Ok(figures)
}

/// `<figure> unit=<pt> em=<pt> source=<file>`; the file name is the rest of
/// the line, so it may contain spaces.
fn parse_figure(rest: &str) -> std::result::Result<TnvmFigure, String> {
    let usage = "expected `figure <figure> unit=<pt> em=<pt> source=<file>`";
    let (name, rest) = rest.split_once(' ').ok_or(usage)?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err(format!("figure name `{name}` is not a letter, digit, `-`, `_`, or `.` sequence"));
    }
    let (fields, source) = rest.split_once("source=").ok_or(usage)?;
    let (mut unit, mut em) = (None, None);
    for field in fields.split_whitespace() {
        let (key, value) = field.split_once('=').ok_or(usage)?;
        let v: f64 = value.parse().map_err(|_| format!("bad length `{value}`"))?;
        match key {
            "unit" => unit = Some(v),
            "em" => em = Some(v),
            _ => return Err(format!("unknown field `{key}`")),
        }
    }
    let (Some(unit_pt), Some(em_pt)) = (unit, em) else {
        return Err(usage.into());
    };
    if unit_pt <= 0.0 || em_pt <= 0.0 {
        return Err("unit and em must be positive".into());
    }
    Ok(TnvmFigure {
        name: name.to_string(),
        unit_pt,
        em_pt,
        source: source.trim().to_string(),
        labels: LabelSizes::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn figures_and_labels() {
        let f = parse_tnvm(
            "tnvm 1\n% comment\nfigure mps unit=28.45274 em=10 source=paper-mps.tnv\n\
             label mps t:A[1] 7.52 6.83 0\nfigure p unit=22.76 em=10.95 source=figures/my peps.tnv\n",
        )
        .unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].labels.get("t:A[1]"), Some([7.52, 6.83, 0.0]));
        assert_eq!(f[1].source, "figures/my peps.tnv");
        assert_eq!(f[1].options().em_pt, 10.95);
    }

    #[test]
    fn errors() {
        let bad = |s: &str| parse_tnvm(s).unwrap_err().to_string();
        assert!(bad("tnvm 2\n").contains("version 2"));
        assert!(bad("figure a unit=1 em=1 source=x\n").contains("starts with"));
        assert!(bad("tnvm 1\nlabel a t:A 1 2 3\n").starts_with("2:1: label of unknown figure"));
        assert!(bad("tnvm 1\nfigure a unit=1 source=x\n").contains("expected"));
        assert!(bad("tnvm 1\nfigure a/b unit=1 em=1 source=x\n").contains("figure name"));
    }
}
