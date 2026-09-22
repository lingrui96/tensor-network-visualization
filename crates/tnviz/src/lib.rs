//! Tensor-network diagrams: the engine and Rust API of the tnv language.
//!
//! A [`Network`] is read from tnv source with [`parse`] or built through its
//! checked operations, printed back as canonical tnv with [`to_tnv`], and
//! placed with [`layout`].  See `docs/language.md` for the language and
//! `docs/architecture.md` for how the stages divide the work.
//!
//! Only the items exported here are the public API; the stages' internals
//! are private to the crate.

mod error;
mod geometry;
mod lang;
mod layout;
mod lighting;
mod md5;
mod model;
mod name;
mod order;
mod registry;
mod tikz;
mod tnvm;
mod value;

#[cfg(feature = "debug-svg")]
mod debug_svg;

pub use error::{Error, Pos, Result};
pub use geometry::{
    Anchor, ArrowGeom, Cap, Crossing, Front, Geometry, GeometryOptions, LabelGeom, LabelOwner, LabelSizes,
    LabelText, LineGeom, LineKind, LineStyle, Patch, Path, Piece, PlaneGeom, Shape, Span, TensorGeom, View,
    geometry,
};
pub use lang::{VERSION, parse, to_tnv};
pub use layout::{
    BondEnd, DEFAULT_SPACING, PlacedBond, PlacedLeg, PlacedTensor, Placement, Route, V3, layout,
};
pub use lighting::expr::{Expr, Program, ps_number};
pub use lighting::{
    ArrowLook, Colour, LabelColour, Lighting, LineLook, Other, PlaneLook, Shading, Shadow, Stroke,
    TensorLook, lighting,
};
pub use model::{
    Camera, Dim, Direction, Group, Index, IndexId, LayoutStmt, LegKey, Network, Plane, PlanePlace, Relation,
    Rule, Scene, Selector, Slot, Tensor, TensorId,
};
pub use name::{Name, NamePattern, SubPattern};
pub use order::{Class, Fragment, Key, Part, Pass, order};
pub use registry::{Category, Target, category};
pub use tikz::{PROTOCOL, tikz};
pub use tnvm::{TNVM_VERSION, TnvmFigure, parse_tnvm};
pub use value::{Attr, Value, merge_attrs};

#[cfg(feature = "debug-svg")]
pub use debug_svg::debug_svg;
