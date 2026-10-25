//! The realm as the chief sees it (docs/tour.md): the generated
//! world and what it stands on, the bridge from the client's entities
//! to it, the chief's mascot, the pointer, the library and the
//! bazaar.

pub mod bazaar;
pub mod bubble;
pub mod chief;
pub mod debug;
pub mod diorama;
pub mod dress;
pub mod generator;
pub mod hamlet;
pub mod hand;
pub mod keeper;
pub mod lantern;
pub mod library;
pub mod meshes;
pub mod moving;
pub mod needs;
pub mod orders;
pub mod palette;
pub mod pick;
pub mod play;
pub mod scene;
pub mod settings;
pub mod sky;
pub mod sound;
pub mod stage;
pub mod stock;
pub mod viewer;
pub mod wardrobe;
pub mod works;

mod label;
#[cfg(test)]
mod testing;

use bevy::prelude::*;

pub use self::sky::{
    Dark, DayPart, Hour, LowSun, North, Sky, WorldCamera,
};
pub use self::stage::{
    Clock, RealmSystems, Stage, Step, WorldSystems,
};
pub use self::stock::Stock;

/// The realm: the generated world and everything that lives in it.
/// Add it after Bevy's default plugins. An app that wants only the
/// scene, with no hamlets, adds [`scene::plugin`] itself.
pub struct RealmPlugin;

impl Plugin for RealmPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            // Later plugins read what the scene puts in.
            scene::plugin,
            // What is drawn in it.
            generator::plugin,
            // What the chief does in it.
            (
                pick::plugin,
                needs::plugin,
                bubble::plugin,
                hand::plugin,
                orders::plugin,
                chief::plugin,
                library::plugin,
                bazaar::plugin,
                diorama::plugin,
                viewer::plugin,
                moving::plugin,
                works::plugin,
                lantern::plugin,
            ),
            // The client's entities, drawn.
            hamlet::plugin,
        ));
    }
}

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
