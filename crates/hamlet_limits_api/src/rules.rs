//! The panes of the rules of the realm and of each hamlet.

use bevy::ecs::entity::Entity;
use hamlet_sdk::AnyView;

use crate::shown;

/// The rules every hamlet of the realm keeps.
pub fn realm() -> AnyView {
    shown(|panes| (panes.realm)())
}

/// The rules of the hamlet `hamlet`: its budget, models and the
/// mayor's leave.
pub fn panel(hamlet: Entity) -> AnyView {
    shown(move |panes| (panes.rules)(hamlet))
}
