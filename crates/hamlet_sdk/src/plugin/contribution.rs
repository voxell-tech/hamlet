//! What a plugin adds belongs to it.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;

/// On what a plugin added: the plugin that added it.
#[derive(Component)]
#[relationship(relationship_target = Contributions)]
pub struct ContributedBy(pub Entity);

/// On a plugin: everything it added. It goes when the plugin does.
#[derive(Component)]
#[relationship_target(relationship = ContributedBy, linked_spawn)]
pub struct Contributions(Vec<Entity>);
