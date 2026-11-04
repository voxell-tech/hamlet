//! The kinds of node a blueprint is made of, each with what it runs.

use hamlet_graph::{Error, NodeKind, Registry};

use crate::eval::Run;
use crate::nodes;

/// A kind of node and the function that runs it.
#[derive(Clone, Copy)]
pub struct BlueprintKind {
    pub kind: NodeKind,
    pub run: fn(&mut Run) -> Result<(), Error>,
}

impl BlueprintKind {
    pub const fn new(
        kind: NodeKind,
        run: fn(&mut Run) -> Result<(), Error>,
    ) -> Self {
        Self { kind, run }
    }
}

/// The built-in kinds, to validate a ruleset against.
pub fn registry() -> Registry {
    let mut registry = Registry::default();
    for (group, kinds) in &nodes::KINDS {
        for kind in *kinds {
            registry.add(group, kind.kind);
        }
    }
    registry
}

pub(crate) fn get(name: &str) -> Option<&'static BlueprintKind> {
    nodes::KINDS
        .iter()
        .flat_map(|(_, kinds)| kinds.iter())
        .find(|kind| kind.kind.name == name)
}
