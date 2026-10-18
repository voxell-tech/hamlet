//! Bringing up a plugin the chief has just let in.

use bevy::prelude::*;
use hamlet_openui::data::{self, DataPanel};
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{
    Asks, Choices, Granted, Installed, Uninstalled,
};
use hamlet_sdk::plugin::{PluginInfo, Status};

use crate::engine::Host;
use crate::limits::Limits;
use crate::start::{self, Setting, Source};

/// Brings up the plugin of this id from its files, with a store of
/// its own if it is code.
pub fn admit(
    installed: On<Installed>,
    plugins: Query<
        (
            Entity,
            &PluginInfo,
            &Asks,
            &Choices,
            Option<&Source>,
            Option<&DataPanel>,
        ),
        Without<Uninstalled>,
    >,
    host: Option<Res<Host>>,
    limits: Res<Limits>,
    agents: Res<AgentList>,
    mut commands: Commands,
) {
    for (plugin, info, asks, choices, source, panel) in &plugins {
        if info.id != installed.plugin {
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
                },
                &mut commands,
            ),
            (Some(_), _, None) => {
                warn!("plugin {}: no engine, not loaded", info.id);
                commands.entity(plugin).insert(Status::Failed);
            }
            (None, Some(panel), _) => {
                data::bring_up(plugin, &info.id, panel, &mut commands)
            }
            (None, None, _) => {}
        }
    }
}
