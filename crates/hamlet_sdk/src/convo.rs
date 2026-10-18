//! The agent the chat is with, and whether the world has one
//! selected.

mod page;

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use hamlet_core::session::SessionKey;

pub use self::page::of_agent;

/// The chat session this window shows and sends to.
#[derive(Resource)]
pub struct Me {
    pub session: Entity,
    pub key: SessionKey,
    pub user: String,
    /// The session with the mayor, whoever the chat is with.
    pub mayor: Entity,
    /// The agent the chat is with.
    pub agent: Entity,
    /// That agent's name. `None` for the mayor.
    pub with: Option<String>,
}

/// Whether an agent is selected in the world: the mayor, its house,
/// another agent or its house.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct AgentSelected(pub bool);
