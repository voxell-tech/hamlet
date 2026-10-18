//! What the user does in a plugin panel.

use bevy::ecs::event::Event;
use bevy::ecs::world::World;

/// Something the user did to a node of a plugin panel.
#[derive(Event)]
pub struct Interaction {
    /// The panel's id in the dock.
    pub panel: String,
    /// The node's id.
    pub node: String,
    pub act: Act,
}

/// What was done.
pub enum Act {
    Click,
    Toggled(bool),
    Typed(String),
}

impl Interaction {
    /// Tells whoever draws the panel that this was done.
    pub fn send(
        world: &mut World,
        panel: &str,
        node: &str,
        act: Act,
    ) {
        world.trigger(Self {
            panel: panel.into(),
            node: node.into(),
            act,
        });
    }
}
