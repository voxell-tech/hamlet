//! A plugin that stopped, and what its panels show then.

use bevy::prelude::*;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::plugin::{Contributions, Status};

use crate::fault::Fault;
use crate::trees::{Drawn, PluginTrees};

/// On the entity of a failed plugin: why it failed.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Failure(pub String);

/// Marks the plugin `plugin` failed, for this reason.
pub fn fail(
    commands: &mut Commands,
    plugin: Entity,
    id: &str,
    fault: &Fault,
) {
    warn!("plugin {id}: {fault}");
    commands
        .entity(plugin)
        .insert((Status::Failed, Failure(fault.to_string())));
}

/// Has each panel of a failed plugin show that it stopped, and why.
pub fn stopped(
    insert: On<Insert, Failure>,
    failed: Query<(&Failure, &Contributions)>,
    panels: Query<&Panel>,
    mut trees: ResMut<PluginTrees>,
) {
    let Ok((Failure(why), contributions)) = failed.get(insert.entity)
    else {
        return;
    };
    for panel in panels.iter_many(contributions.iter()) {
        let drawn = Drawn::Stopped(why.clone());
        if !trees.shows(&panel.id, &drawn) {
            trees.set(&panel.id, drawn);
        }
    }
}
