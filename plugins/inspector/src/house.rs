//! The house of the agent the chat is with.

use bevy::ecs::world::World;
use hamlet_core::session::{self, Seat};
use hamlet_home::Home;
use hamlet_sdk::HomeDir;

use crate::me::Me;

/// The house of the selected agent: its root and its workspace.
pub(crate) fn house(world: &World) -> Home {
    let home = &world.resource::<HomeDir>().0;
    let me = world.resource::<Me>();
    session::house_at(home, world.get::<Seat>(me.session))
}
