//! Bringing up a plugin the chief has just let in.

use std::path::Path;

use bevy::prelude::*;
use hamlet_openui::data::{self, DataPanel};
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{
    Asks, Choices, Granted, Installed, Uninstalled,
};
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{
    Blocked, Manifest, PluginInfo, Status, needs, present,
};

use crate::engine::Host;
use crate::limits::Limits;
use crate::reload::keep;
use crate::start::{self, Setting, Source};

/// Brings up the plugin of this id from its files, with a store of
/// its own if it is code, unless a plugin it needs is not there or
/// not running.
pub fn admit(
    installed: On<Installed>,
    plugins: Query<
        (
            Entity,
            &PluginInfo,
            &Manifest,
            &Asks,
            &Choices,
            Option<&Source>,
            Option<&DataPanel>,
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
    let present = present(everyone.iter().map(
        |(info, status, uninstalled)| (info, *status, uninstalled),
    ));
    for (plugin, info, manifest, asks, choices, source, panel) in
        &plugins
    {
        if info.id != installed.plugin {
            continue;
        }
        if source.is_some()
            && let Some(why) = needs::unmet(&manifest.needs, &present)
        {
            warn!("plugin {}: not started, {why}", info.id);
            commands
                .entity(plugin)
                .insert((Status::Disabled, Blocked(why.to_string())));
            continue;
        }
        commands.entity(plugin).insert(Status::Starting);
        match (source, panel, host.as_deref()) {
            (Some(source), _, Some(host)) => start::bring_up(
                plugin,
                info,
                source,
                &Granted::of(asks, choices),
                &Setting {
                    host,
                    limits: &limits,
                    agents: &agents,
                    papers: &papers,
                },
                &[],
                &mut commands,
            ),
            (Some(_), _, None) => {
                warn!("plugin {}: no engine, not loaded", info.id);
                commands.entity(plugin).insert(Status::Failed);
            }
            (None, Some(panel), _) => {
                data::bring_up(
                    plugin,
                    &info.id,
                    panel,
                    &mut commands,
                );
                let folder =
                    panel.file.parent().unwrap_or(Path::new(""));
                commands
                    .entity(plugin)
                    .insert(keep::remember(folder));
            }
            (None, None, _) => {}
        }
    }
}
