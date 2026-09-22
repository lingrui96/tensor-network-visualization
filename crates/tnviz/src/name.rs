use std::fmt;

/// The name of a tensor or an index: a base with optional integer
/// subscripts, such as `A`, `A[3]`, or `T[1,2]`.
///
/// Names order by base, then by subscripts numerically, so `A[2]` comes
/// before `A[10]`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name {
    pub base: String,
    pub subscripts: Vec<i64>,
}

impl Name {
    pub fn new(base: impl Into<String>, subscripts: impl Into<Vec<i64>>) -> Self {
        Name { base: base.into(), subscripts: subscripts.into() }
    }

    pub fn plain(base: impl Into<String>) -> Self {
        Name { base: base.into(), subscripts: Vec::new() }
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.base)?;
        if !self.subscripts.is_empty() {
            f.write_str("[")?;
            for (k, s) in self.subscripts.iter().enumerate() {
                if k > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{s}")?;
            }
            f.write_str("]")?;
        }
        Ok(())
    }
}

/// One subscript position of a name pattern.
#[derive(Clone, Debug, PartialEq)]
pub enum SubPattern {
    Exact(i64),
    /// An inclusive range.
    Range(i64, i64),
    /// `*`: any value.
    Any,
}

/// A pattern over names, such as `A[*]`, `T[*, 1]`, or `A[2..4]`, with an
/// optional prime level for indices.
#[derive(Clone, Debug, PartialEq)]
pub struct NamePattern {
    pub base: String,
    pub subscripts: Vec<SubPattern>,
    pub prime: u32,
}

impl NamePattern {
    pub fn exact(name: &Name, prime: u32) -> Self {
        NamePattern {
            base: name.base.clone(),
            subscripts: name.subscripts.iter().map(|&s| SubPattern::Exact(s)).collect(),
            prime,
        }
    }

    /// Whether the pattern names exactly one thing.
    pub fn is_exact(&self) -> bool {
        self.subscripts.iter().all(|s| matches!(s, SubPattern::Exact(_)))
    }

    /// Whether the pattern is a bare identifier, which may also name a group.
    pub fn is_bare(&self) -> bool {
        self.subscripts.is_empty() && self.prime == 0
    }

    /// Whether the pattern matches a name.  A lone `*`, as in `A[*]`,
    /// matches every subscripted `A`, whatever the number of subscripts.
    pub fn matches(&self, name: &Name, prime: u32) -> bool {
        if self.subscripts == [SubPattern::Any] {
            return self.base == name.base && self.prime == prime && !name.subscripts.is_empty();
        }
        self.base == name.base
            && self.prime == prime
            && self.subscripts.len() == name.subscripts.len()
            && self.subscripts.iter().zip(&name.subscripts).all(|(p, &s)| match *p {
                SubPattern::Exact(e) => e == s,
                SubPattern::Range(a, b) => a <= s && s <= b,
                SubPattern::Any => true,
            })
    }
}

impl fmt::Display for NamePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.base)?;
        if !self.subscripts.is_empty() {
            f.write_str("[")?;
            for (k, s) in self.subscripts.iter().enumerate() {
                if k > 0 {
                    f.write_str(",")?;
                }
                match s {
                    SubPattern::Exact(e) => write!(f, "{e}")?,
                    SubPattern::Range(a, b) => write!(f, "{a}..{b}")?,
                    SubPattern::Any => f.write_str("*")?,
                }
            }
            f.write_str("]")?;
        }
        for _ in 0..self.prime {
            f.write_str("'")?;
        }
        Ok(())
    }
}
