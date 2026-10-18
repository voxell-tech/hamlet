//! Starting an installed plugin again when it is enabled.

use bevy::prelude::*;
use hamlet_sdk::plugin::{Contributions, Enable, PluginInfo, Status};

use crate::engine::Host;
use crate::limits::Limits;
use crate::start::{self, Source};

/// Brings up again the disabled or failed plugin of this id, from
/// its file, with a store of its own.
pub fn restart(
    enable: On<Enable>,
    plugins: Query<(Entity, &PluginInfo, &Status, &Source)>,
    host: Option<Res<Host>>,
    limits: Res<Limits>,
    mut commands: Commands,
) {
    let Some(host) = host else {
        return;
    };
    for (plugin, info, status, source) in &plugins {
        if info.id != enable.0
            || !matches!(status, Status::Disabled | Status::Failed)
        {
            continue;
        }
        commands
            .entity(plugin)
            .despawn_related::<Contributions>()
            .insert(Status::Starting);
        start::bring_up(
            plugin,
            info,
            &source.0,
            &host,
            &limits,
            &mut commands,
        );
    }
}
