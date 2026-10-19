//! Starting an installed plugin again when it is enabled.

use bevy::prelude::*;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{Granted, Uninstalled};
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{
    Blocked, Contributions, Enable, Manifest, PluginInfo, Status,
    needs, present,
};

use crate::engine::Host;
use crate::instance::Instance;
use crate::limits::Limits;
use crate::start::{self, Setting, Source};
use crate::stop::{Automatic, Retry, Tries};

/// Brings up again the disabled or failed plugin of this id, from
/// its file, with a store of its own, unless a plugin it needs is not
/// there or not running.
pub fn restart(
    enable: On<Enable>,
    plugins: Query<
        (
            Entity,
            &PluginInfo,
            &Manifest,
            &Status,
            &Source,
            &Granted,
            Option<&Instance>,
            Has<Automatic>,
        ),
        Without<Uninstalled>,
    >,
    everyone: Query<(&PluginInfo, &Status, Has<Uninstalled>)>,
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
    let present = present(everyone.iter().map(
        |(info, status, uninstalled)| (info, *status, uninstalled),
    ));
    for (
        plugin,
        info,
        manifest,
        status,
        source,
        granted,
        instance,
        automatic,
    ) in &plugins
    {
        if info.id != enable.0
            || !matches!(status, Status::Disabled | Status::Failed)
        {
            continue;
        }
        // The chief starting a plugin gives it its retries back.
        commands.entity(plugin).remove::<(Retry, Automatic)>();
        if !automatic {
            commands.entity(plugin).remove::<Tries>();
        }
        if let Some(why) = needs::unmet(&manifest.needs, &present) {
            warn!("plugin {}: not started, {why}", info.id);
            commands.entity(plugin).insert(Blocked(why.to_string()));
            continue;
        }
        commands
            .entity(plugin)
            .despawn_related::<Contributions>()
            .insert(Status::Starting)
            .remove::<Blocked>();
        start::bring_up(
            plugin,
            info,
            &manifest.exports,
            source,
            granted,
            &setting,
            instance.map_or(&[], Instance::state),
            &mut commands,
        );
    }
}
