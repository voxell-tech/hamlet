//! Starting an installed plugin again when it is enabled.

use bevy::prelude::*;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{Granted, Uninstalled};
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{Contributions, Enable, PluginInfo, Status};

use crate::engine::Host;
use crate::instance::Instance;
use crate::limits::Limits;
use crate::start::{self, Setting, Source};

/// Brings up again the disabled or failed plugin of this id, from
/// its file, with a store of its own.
pub fn restart(
    enable: On<Enable>,
    plugins: Query<
        (
            Entity,
            &PluginInfo,
            &Status,
            &Source,
            &Granted,
            Option<&Instance>,
        ),
        Without<Uninstalled>,
    >,
    host: Option<Res<Host>>,
    limits: Res<Limits>,
    agents: Res<AgentList>,
    papers: Res<Papers>,
    mut commands: Commands,
) {
    let Some(host) = host else {
        return;
    };
    let setting = Setting {
        host: &host,
        limits: &limits,
        agents: &agents,
        papers: &papers,
    };
    for (plugin, info, status, source, granted, instance) in &plugins
    {
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
            source,
            granted,
            &setting,
            instance.map_or(&[], Instance::state),
            &mut commands,
        );
    }
}
