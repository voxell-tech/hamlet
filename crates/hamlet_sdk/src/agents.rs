//! The agents and their hamlets, as a plugin that holds
//! `read.agents` reads them.

use bevy::prelude::*;

/// The systems that write [`AgentList`] and [`SelectedAgent`].
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Kept;

/// A living agent and the name of its hamlet.
#[derive(Clone, PartialEq, Debug)]
pub struct Agent {
    pub name: String,
    pub hamlet: String,
}

/// The living agents, by name, each with its hamlet.
#[derive(Resource, Default, PartialEq, Clone, Debug)]
pub struct AgentList(pub Vec<Agent>);

/// The name of the agent selected in the world, if one is.
#[derive(Resource, Default, PartialEq, Clone, Debug)]
pub struct SelectedAgent(pub Option<String>);
