//! A running plugin.

use bevy::ecs::component::Component;
use wasmtime::Store;

/// On the entity of an installed plugin: its store.
#[derive(Component)]
pub struct Instance(pub Store<()>);
