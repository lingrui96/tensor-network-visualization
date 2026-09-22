use std::fmt;

/// A position in tnv source text, 1-based.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// An error in tnv source, or in building a network.
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    /// Where the error was found; `None` for errors raised through the API.
    pub pos: Option<Pos>,
    pub message: String,
}

impl Error {
    pub fn at(pos: Pos, message: impl Into<String>) -> Self {
        Error { pos: Some(pos), message: message.into() }
    }

    pub fn new(message: impl Into<String>) -> Self {
        Error { pos: None, message: message.into() }
    }

    /// Attach a position unless the error already has one.
    pub fn or_at(mut self, pos: Pos) -> Self {
        self.pos.get_or_insert(pos);
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.pos {
            Some(pos) => write!(f, "{pos}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
