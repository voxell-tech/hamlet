//! The agent the chat is with, and whether the world has one
//! selected.

mod page;
mod send;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use hamlet_core::session::SessionKey;

pub use self::page::of_agent;
pub use self::send::{address, send_as_chief};

pub fn plugin(app: &mut App) {
    app.insert_resource(ChiefName::from_env());
}

/// The name of this window's chief: `HAMLET_USER`, then `USER`.
#[derive(Resource, Clone, PartialEq, Debug)]
pub struct ChiefName(pub String);

impl ChiefName {
    fn from_env() -> Self {
        Self(
            std::env::var("HAMLET_USER")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "user".into()),
        )
    }
}

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
