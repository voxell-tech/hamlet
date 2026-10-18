//! A plugin that is a panel in the language and no code.

use std::path::PathBuf;

use bevy::prelude::*;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::plugin::{
    ContributedBy, Contributions, Enable, PluginInfo, Status,
};
use hamlet_sdk::tree::id::dock_id;
use hamlet_sdk::tree::view;

use crate::source::Source;

/// The id of a data plugin's panel, before the plugin's id is put in
/// front of it.
pub const PANEL: &str = "main";

/// On the entity of a data plugin: the panel it adds.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct DataPanel {
    /// The file of the program.
    pub file: PathBuf,
    pub title: String,
    pub icon: Glyph,
}

/// Reads the program of the data plugin `plugin` and adds its panel.
/// A plugin whose file cannot be read is marked failed.
pub fn bring_up(
    plugin: Entity,
    id: &str,
    data: &DataPanel,
    commands: &mut Commands,
) {
    match std::fs::read_to_string(&data.file) {
        Ok(text) => {
            commands.spawn((
                Panel::by_id(
                    dock_id(id, PANEL),
                    &data.title,
                    data.icon,
                    view::panel,
                ),
                ContributedBy(plugin),
                Source(text),
            ));
            commands.entity(plugin).insert(Status::Running);
        }
        Err(why) => {
            warn!("plugin {id}: {}: {why}", data.file.display());
            commands.entity(plugin).insert(Status::Failed);
        }
    }
}

/// Brings up again the disabled or failed data plugin of this id,
/// from its file.
pub(crate) fn restart(
    enable: On<Enable>,
    plugins: Query<(Entity, &PluginInfo, &Status, &DataPanel)>,
    mut commands: Commands,
) {
    for (plugin, info, status, data) in &plugins {
        if info.id != enable.0
            || !matches!(status, Status::Disabled | Status::Failed)
        {
            continue;
        }
        commands
            .entity(plugin)
            .despawn_related::<Contributions>()
            .insert(Status::Starting);
        bring_up(plugin, &info.id, data, &mut commands);
    }
}
