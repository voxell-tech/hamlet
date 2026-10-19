//! A plugin that stopped, and what its panels show then.

use bevy::prelude::*;
use hamlet_openui::source::Program;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::plugin::{Contributions, Enable, PluginInfo, Status};
use hamlet_sdk::tree::store::{Drawn, PluginTrees};

use crate::fault::Fault;
use crate::limits::Limits;

/// On the entity of a failed plugin: why it failed.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Failure(pub String);

/// On a failed plugin the host will start again by itself: the
/// second, by the app's clock, it does.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct Retry(pub f32);

/// On a plugin: the times the host started it again by itself since
/// the chief last did.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct Tries(pub u32);

/// On a plugin the host is starting again by itself, until it is
/// up.
#[derive(Component)]
pub struct Automatic;

/// Plans the automatic restart of a plugin that failed, if it has any
/// left and there is a clock to wait by.
pub fn schedule(
    insert: On<Insert, Failure>,
    tries: Query<Option<&Tries>>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    mut commands: Commands,
) {
    let (Some(time), Ok(tries)) = (time, tries.get(insert.entity))
    else {
        return;
    };
    if tries.map_or(0, |tries| tries.0) < limits.restart_retries {
        let wait = limits.restart_backoff_ms as f32 / 1000.0;
        commands
            .entity(insert.entity)
            .insert(Retry(time.elapsed_secs() + wait));
    }
}

/// Starts again, with `Enable`, each failed plugin whose wait is
/// over.
pub fn retry(
    plugins: Query<(
        Entity,
        &PluginInfo,
        &Status,
        &Retry,
        Option<&Tries>,
    )>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (plugin, info, status, retry, tries) in &plugins {
        if *status != Status::Failed || time.elapsed_secs() < retry.0
        {
            continue;
        }
        info!("plugin {}: starting again", info.id);
        commands.entity(plugin).remove::<Retry>().insert((
            Tries(tries.map_or(0, |tries| tries.0) + 1),
            Automatic,
        ));
        commands.trigger(Enable(info.id.clone()));
    }
}

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
    panels: Query<(Entity, &Panel)>,
    mut trees: ResMut<PluginTrees>,
    mut commands: Commands,
) {
    let Ok((Failure(why), contributions)) = failed.get(insert.entity)
    else {
        return;
    };
    for (entity, panel) in panels.iter_many(contributions.iter()) {
        let drawn = Drawn::Stopped(why.clone());
        if !trees.shows(&panel.id, &drawn) {
            trees.set(&panel.id, drawn);
        }
        // A program would draw over the placeholder.
        commands.entity(entity).remove::<Program>();
    }
}
