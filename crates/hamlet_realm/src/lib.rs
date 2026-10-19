//! The realm as the chief sees it (docs/tour.md): the generated
//! world and what it stands on, the bridge from the core's entities
//! to it, the chief's mascot, the pointer and the library.

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

/// The realm: the generated world and everything that lives in it.
/// Add it after Bevy's default plugins. An app that wants only the
/// scene, with no hamlets, adds the plugins of `settings`,
/// `palette`, `play`, `sound`, `debug`, `sky`, `stage`, `keeper` and
/// `stock` itself.
pub struct RealmPlugin;

impl Plugin for RealmPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            // The scene: tuning, colours, camera, clock, robots and
            // the shared materials. Later plugins read them.
            (
                settings::plugin,
                palette::plugin,
                play::plugin,
                sound::plugin,
                debug::plugin,
                sky::plugin,
                stage::plugin,
                keeper::plugin,
                stock::plugin,
            ),
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
                moving::plugin,
                works::plugin,
                lantern::plugin,
            ),
            // The core's entities, drawn, and what a plugin reads of
            // them.
            (hamlet::plugin, survey::plugin),
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
