//! Procedural graphs: rules for placing a building's parts, written
//! as nodes and wires, and the evaluator that runs them.
//!
//! An author edits a [`Sheet`](hamlet_graph::Sheet) and exports a
//! [`Ruleset`](hamlet_graph::Ruleset). The game runs a ruleset with
//! [`evaluate`] and a set of [`Seeds`], and gets back [`Object`]s of
//! named assets.

mod bundle;
mod eval;
mod field;
mod kind;
mod nodes;
mod seed;
mod turn;
mod value;

pub use bundle::Bundle;
pub use eval::{Run, Seeds, evaluate, evaluate_partly};
pub use field::{Field, Measure, Noise, NoiseKind, Op, Random};
pub use kind::{BlueprintKind, registry};
pub use nodes::upgrade;
pub use seed::Seed;
pub use value::{
    Curve, Frame, Object, ObjectId, TransformValue, Value,
};
