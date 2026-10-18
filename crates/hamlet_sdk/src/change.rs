//! The edit an approval holds, drawn the way the chat draws it.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use hamlet_core::tools::Change;

use crate::AnyView;

/// Draws the edit at entry `id` of the conversation `session`. The
/// window sets it, so that a plugin shows an edit as the chat does.
#[derive(Resource, Clone, Copy)]
pub struct DrawChange(pub fn(Entity, u64, Change) -> AnyView);
