//! Node graphs as data: the graphs a file holds, the edits an editor
//! makes to them, the kinds of node they may use, and the checks that
//! need no more than those kinds' descriptions.
//!
//! What a kind does is left to the crate that evaluates the graph.

mod catalogue;
mod check;
mod edit;
mod graph;
mod node;
mod value;

pub use catalogue::{Asset, BODY, Catalogue, Paint, Shape, Socket};
pub use check::{
    Error, GraphError, PortInfo, order, ports, validate,
};
pub use edit::{Edit, EditRefusal};
pub use graph::{
    Book, Does, Graph, GraphInput, GraphOutput, Group, Node, NodeId,
    Port, Roll, Ruleset, Scope, Setting, Sheet, Spread, Vary, Wire,
    to_ron,
};
pub use node::{Initial, Listed, NodeKind, PortSpec, Registry};
pub use value::{Constant, ValueType};
