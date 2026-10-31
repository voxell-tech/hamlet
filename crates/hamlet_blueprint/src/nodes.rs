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
    ("Objects", leaves::KINDS),
    ("Transforms", make::KINDS),
    ("Modifiers", modify::KINDS),
    ("Curves", curves::KINDS),
    ("Fields", fields::KINDS),
    ("Math", math::KINDS),
];

const TRANSFORMS_IN: PortSpec = PortSpec::new(
    "transforms",
    ValueType::Transforms,
    "The transforms to work on",
);
const TRANSFORMS_OUT: PortSpec =
    PortSpec::new("transforms", ValueType::Transforms, "The result");
