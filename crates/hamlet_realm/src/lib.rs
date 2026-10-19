//! The realm as the chief sees it (docs/tour.md): the generated
//! world and what it stands on, the bridge from the core's entities
//! to it, the chief's mascot, the pointer and the library. A
//! module's `plugin` adds its own systems, and an app adds the
//! modules it draws, the scene's first.

pub mod bubble;
pub mod chief;
pub mod debug;
pub mod generator;
pub mod hamlet;
pub mod hand;
pub mod keeper;
pub mod label;
pub mod lantern;
pub mod library;
pub mod meshes;
pub mod moving;
pub mod needs;
pub mod orders;
pub mod pick;
pub mod play;
pub mod settings;
pub mod sky;
pub mod sound;
pub mod stage;
pub mod stock;
pub mod survey;
pub mod works;

use bevy::prelude::*;
pub use hamlet_realm_api::palette;

pub use self::sky::{Dark, DayPart, Hour, LowSun, Sky, WorldCamera};
pub use self::stage::{Clock, Stage, Step, WorldSystems};
pub use self::stock::Stock;

/// Puts `child` under `parent`, or despawns it if the parent has
/// gone, as a house does with its town.
pub(crate) fn adopt(
    commands: &mut Commands,
    parent: Entity,
    child: Entity,
) {
    commands.queue(move |world: &mut World| {
        if world.get_entity(parent).is_ok() {
            if let Ok(mut child) = world.get_entity_mut(child) {
                child.insert(ChildOf(parent));
            }
        } else if let Ok(child) = world.get_entity_mut(child) {
            child.despawn();
        }
    });
}
