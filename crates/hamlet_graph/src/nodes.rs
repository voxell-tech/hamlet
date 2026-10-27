//! The built-in kinds of node.

mod curves;
mod fields;
mod leaves;
mod make;
mod math;
mod modify;

use crate::node::{NodeKind, PortSpec};
use crate::value::ValueType;

/// The built-in kinds, by the group each is listed under.
pub(crate) const KINDS: [(&str, &[NodeKind]); 6] = [
    ("Curves", curves::KINDS),
    ("Placements", make::KINDS),
    ("Modifiers", modify::KINDS),
    ("Fields", fields::KINDS),
    ("Math", math::KINDS),
    ("Output", leaves::KINDS),
];

const PLACEMENTS_IN: PortSpec = PortSpec::new(
    "placements",
    ValueType::Placements,
    "The placements to work on",
);
const PLACEMENTS_OUT: PortSpec =
    PortSpec::new("placements", ValueType::Placements, "The result");
