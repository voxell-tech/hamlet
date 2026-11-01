//! Node graphs as data: the graphs a file holds, the edits an editor
//! makes to them, the kinds of node they may use, and the checks that
//! need no more than those kinds' descriptions.

mod catalogue;
mod check;
mod edit;
mod fragment;
mod graph;
mod grows;
mod node;
mod value;

pub use catalogue::{Asset, BODY, Catalogue, Paint, Shape, Socket};
pub use check::{
    Error, GraphError, PortInfo, order, ports, validate,
};
pub use edit::{Edit, EditRefusal};
pub use fragment::Fragment;
pub use graph::{
    Book, Does, Graph, GraphInput, GraphOutput, Group, Node, NodeId,
    Port, Roll, Ruleset, Scope, Setting, Sheet, Spread, Vary, Wire,
    to_ron,
};
pub use grows::pack;
pub use node::{Initial, Listed, NodeKind, PortSpec, Registry};
pub use value::{Constant, ValueType};
