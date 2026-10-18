//! Plugins as the Plugins page shows them after a reload: one whose
//! new version did not start, one that waits for the chief's answer
//! and one that reloaded. Two more show what is held back for a
//! plugin it needs.

use bevy::ecs::world::World;
use hamlet_sdk::plugin::{Blocked, Manifest, PluginInfo, Status};
use hamlet_sdk::reload::Reload;

fn info(id: &str, name: &str, version: &str) -> PluginInfo {
    PluginInfo {
        id: id.into(),
        name: name.into(),
        version: version.into(),
        builtin: false,
        essential: false,
    }
}

pub(super) fn put(world: &mut World) {
    world.spawn((
        info("roster", "Roster", "0.2.0"),
        Status::Running,
        Reload {
            note: Some(
                "could not load: built for interface 0.1.0, this \
                 Hamlet speaks 0.8.0"
                    .into(),
            ),
            rollback: true,
        },
    ));
    world.spawn((
        info("almanac", "Almanac", "0.3.0"),
        Status::Running,
        Reload {
            note: Some(
                "The new version asks for more. It loads when you \
                 answer."
                    .into(),
            ),
            rollback: true,
        },
    ));
    world.spawn((
        info("tides", "Tides", "1.2.0"),
        Status::Running,
        Reload::default(),
    ));
    // Notes was switched off, and Clipper, which needs it, with it.
    world.spawn((
        info("notes", "Notes", "0.1.0"),
        Manifest::built_in("notes"),
        Status::Disabled,
    ));
    world.spawn((
        info("clipper", "Clipper", "0.1.0"),
        Manifest::built_in("clipper").needs("notes", ">=0.1, <1"),
        Status::Disabled,
        Blocked("needs `notes`, which is switched off".into()),
    ));
}
