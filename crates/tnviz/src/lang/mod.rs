//! The tnv language: lexing, parsing, lowering to a [`Network`], and
//! printing a network back in canonical form.
//!
//! [`Network`]: crate::model::Network

pub mod ast;
mod lexer;
mod lower;
mod parser;
mod print;

pub use lower::VERSION;
pub use parser::parse as parse_statements;
pub use print::to_tnv;

use crate::error::Result;
use crate::model::Network;

/// Read tnv source into a network.
pub fn parse(src: &str) -> Result<Network> {
    lower::lower(&parser::parse(src)?)
}
