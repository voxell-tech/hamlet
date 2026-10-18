//! Bringing a plugin up: instantiate it, run its `entry` and spawn
//! the panels it declared.

use std::path::PathBuf;

use bevy::prelude::*;
use hamlet_sdk::plugin::{PluginInfo, Status};

use crate::agents::AgentList;
use crate::draw::Dirty;
use crate::engine::Host;
use crate::grant::Granted;
use crate::inbox::Inbox;
use crate::instance::Instance;
use crate::limits::Limits;
use crate::stop::{self, Failure};
use crate::{panel, run};

/// On the entity of an installed plugin: the file of its component.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Source(pub PathBuf);

/// What a plugin is started with.
pub struct Setting<'a> {
    pub host: &'a Host,
    pub limits: &'a Limits,
    pub agents: &'a AgentList,
}

/// Instantiates the component in `source` and runs its `entry`, for
/// the plugin entity `plugin`. A plugin that does not start is
/// marked failed.
pub fn bring_up(
    plugin: Entity,
    info: &PluginInfo,
    source: &Source,
    granted: &Granted,
    setting: &Setting,
    commands: &mut Commands,
) {
    let Setting {
        host,
        limits,
        agents,
    } = setting;
    let started = Instance::from_file(&host.0, &source.0, limits)
        .and_then(|instance| {
            let mut instance = instance.named(&info.id);
            instance.see(granted, agents);
            let output = instance.entry(limits)?;
            output
                .logs
                .iter()
                .for_each(|line| run::say(&info.id, line));
            Ok((instance, output.panels))
        });
    match started {
        Ok((instance, declared)) => {
            let shown =
                panel::spawn(plugin, &info.id, &declared, commands);
            commands
                .entity(plugin)
                .insert((
                    instance,
                    Status::Running,
                    Inbox::default(),
                    Dirty::of(shown),
                ))
                .remove::<Failure>();
        }
        Err(fault) => stop::fail(commands, plugin, &info.id, &fault),
    }
}
