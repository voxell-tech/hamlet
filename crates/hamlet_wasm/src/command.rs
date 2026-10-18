//! The commands of an installed plugin, as entities linked to it.

use bevy::prelude::*;
use hamlet_sdk::command::Command;
use hamlet_sdk::plugin::{Entry, Exports};

use crate::host::Offered;

/// Spawns the commands `declared` that the plugin `id` lists in its
/// `exports`, as contributions of `plugin`. The others are dropped
/// with a warning, since nothing outside the lists is reachable.
pub fn spawn(
    plugin: Entity,
    id: &str,
    exports: &Exports,
    declared: &[Offered],
    commands: &mut Commands,
) {
    let mut entry = Entry::default();
    for offered in declared {
        if exports.commands.contains(&offered.id) {
            entry.forwarded(&offered.id, &offered.title, None);
        } else {
            warn!(
                "plugin {id}: the command `{}` is not in its \
                 exports, dropped",
                offered.id
            );
        }
    }
    entry.spawn(plugin, commands);
}

/// Makes the commands of `plugin` the ones `declared` that it lists
/// in its `exports`, given the `held` ones it has.
pub fn replace(
    plugin: Entity,
    id: &str,
    exports: &Exports,
    held: &[Entity],
    declared: &[Offered],
    commands: &mut Commands,
) {
    for entity in held {
        commands.entity(*entity).despawn();
    }
    spawn(plugin, id, exports, declared, commands);
}

/// The commands among `contributions`, from `commands`.
pub fn held(
    contributions: impl IntoIterator<Item = Entity>,
    commands: &Query<(), With<Command>>,
) -> Vec<Entity> {
    contributions
        .into_iter()
        .filter(|entity| commands.contains(*entity))
        .collect()
}
