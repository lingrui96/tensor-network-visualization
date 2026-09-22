//! Tensor-network diagrams.
//!
//! This crate holds the tnv language and its data model.  A [`Network`] is
//! built either from tnv source with [`parse`] or through its API, and
//! [`to_tnv`] prints any network in canonical tnv.  See `docs/language.md`
//! for the language.

pub mod error;
pub mod lang;
pub mod model;
pub mod name;
pub mod value;

pub use error::{Error, Pos, Result};
pub use lang::{parse, to_tnv};
pub use model::{Index, IndexId, Network, Tensor, TensorId};
pub use name::Name;
