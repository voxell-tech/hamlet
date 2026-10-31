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
    ("Spots", make::KINDS),
    ("Modifiers", modify::KINDS),
    ("Fields", fields::KINDS),
    ("Math", math::KINDS),
    ("Output", leaves::KINDS),
];

const SPOTS_IN: PortSpec =
    PortSpec::new("spots", ValueType::Spots, "The spots to work on");
const SPOTS_OUT: PortSpec =
    PortSpec::new("spots", ValueType::Spots, "The result");
