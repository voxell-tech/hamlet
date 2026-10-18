//! What a node of the drawn tree does when the user touches it.

use std::collections::HashMap;

use bevy::prelude::*;
use openui_lang::ActionPlan;

/// The `$variable` a node writes to, and the action it runs.
#[derive(Clone, PartialEq, Default, Debug)]
pub struct Wire {
    pub binding: Option<String>,
    pub action: Option<ActionPlan>,
}

/// On a panel's entity: the [`Wire`] of each node that can be
/// touched, by the node's id.
#[derive(Component, Default, Debug)]
pub struct Wires(pub HashMap<String, Wire>);
