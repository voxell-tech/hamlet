//! Procedural graphs: rules for placing a building's parts, written
//! as nodes and wires, and the evaluator that runs them.
//!
//! An author edits a [`Sheet`] and exports a [`Ruleset`]. The game
//! runs a ruleset with [`evaluate`] and a set of [`Seeds`], and gets
//! back [`Instance`]s of named assets.

mod catalogue;
mod edit;
mod eval;
mod field;
mod graph;
mod node;
mod nodes;
mod seed;
mod turn;
mod value;

pub use catalogue::{Asset, BODY, Catalogue, Paint, Shape};
pub use edit::{Edit, EditRefusal};
pub use eval::{
    GraphProblem, PortInfo, Problem, Run, Seeds, evaluate, ports,
    validate,
};
pub use field::{Field, Measure, Noise, NoiseKind, Op, Random};
pub use graph::{
    Book, Does, Graph, GraphInput, GraphOutput, Group, Node, NodeId,
    Port, Roll, Ruleset, Scope, Setting, Sheet, Spread, Vary, Wire,
};
pub use node::{Initial, NodeKind, PortSpec, Registry};
pub use seed::Seed;
pub use value::{
    Constant, Curve, Frame, Instance, Placement, PlacementId, Value,
    ValueType,
};
