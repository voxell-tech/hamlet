//! A chief in the world (docs/chief.md): who they are, the agent
//! their mascot keeps company and their look.

pub mod look;

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;

pub use self::look::{Cloak, Hat, Holds, Light, Look};

/// A human the world knows, by the name they sign in with.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Chief {
    pub user: String,
}

/// The agent a chief is talking to, whose side its mascot keeps.
/// Nobody while it is at home.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Beside(pub Option<Entity>);

/// The chief called `user`, if the world knows one.
pub fn named(world: &mut World, user: &str) -> Option<Entity> {
    world
        .query::<(Entity, &Chief)>()
        .iter(world)
        .find(|(_, chief)| chief.user == user)
        .map(|(entity, _)| entity)
}
