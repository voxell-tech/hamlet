//! The built-in kinds of node.

mod curves;
mod fields;
mod leaves;
mod make;
mod modify;

use crate::node::{NodeKind, PortSpec};
use crate::value::ValueType;

pub(crate) const KINDS: [&[NodeKind]; 5] = [
    curves::KINDS,
    make::KINDS,
    modify::KINDS,
    fields::KINDS,
    leaves::KINDS,
];

const PLACEMENTS_IN: PortSpec = PortSpec::new(
    "placements",
    ValueType::Placements,
    "The placements to work on",
);
const PLACEMENTS_OUT: PortSpec =
    PortSpec::new("placements", ValueType::Placements, "The result");
