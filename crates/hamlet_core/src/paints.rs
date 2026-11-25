//! The colour each agent and chief wears in the world, which the
//! panels show them by.

use std::collections::HashMap;

use bevy::app::App;
use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;

pub fn plugin(app: &mut App) {
    app.init_resource::<Paints>();
}

/// The colour each agent wears in the world, by its entity, and each
/// chief, by name.
#[derive(Resource, Default, PartialEq, Debug)]
pub struct Paints {
    pub agents: HashMap<Entity, Color>,
    pub chiefs: HashMap<String, Color>,
}
