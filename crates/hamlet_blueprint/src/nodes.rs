//! The built-in kinds of node.

mod curves;
mod fields;
mod leaves;
mod make;
mod math;
mod modify;

use hamlet_graph::{PortSpec, ValueType};

pub use self::leaves::upgrade;
use crate::kind::BlueprintKind;

/// The built-in kinds, by the group each is listed under.
pub(crate) static KINDS: [(&str, &[BlueprintKind]); 6] = [
    ("Objects", leaves::KINDS),
    ("Copies", make::KINDS),
    ("Modifiers", modify::KINDS),
    ("Curves", curves::KINDS),
    ("Fields", fields::KINDS),
    ("Math", math::KINDS),
];

const OBJECTS_IN: PortSpec =
    PortSpec::new("objects", ValueType::Objects, "The objects");
const OBJECTS_OUT: PortSpec =
    PortSpec::new("objects", ValueType::Objects, "The objects made");
