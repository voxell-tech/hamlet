//! The rules' record of a hamlet and of the realm as a plugin reads
//! them: the names of the first hamlet and of a mayor, the work a
//! hamlet is pointed at, its votes, the skills an agent could be
//! given and the chief's commands.

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::world::World;
use hamlet_core::agents::{LivesIn, Settlement};
pub use hamlet_rules::budget::format_tokens;
pub use hamlet_rules::chief::{Target, switch};
pub use hamlet_rules::commands::RESUME;
pub use hamlet_rules::gifts::Offer;
pub use hamlet_rules::hamlet::{HOME, Hamlet, MAYOR};
pub use hamlet_rules::realm::{Origin, Realm};
pub use hamlet_rules::vote::{self, Standing};
pub use hamlet_rules::work::{Mode, Work, auto_in};

/// The skills `agent` could be given.
pub fn offers(world: &World, agent: Entity) -> Vec<Offer> {
    let Some(name) = world.get::<Name>(agent) else {
        return Vec::new();
    };
    world
        .get::<LivesIn>(agent)
        .and_then(|lives| world.get::<Settlement>(lives.0))
        .map(|settlement| settlement.record().offers(name.as_str()))
        .unwrap_or_default()
}
