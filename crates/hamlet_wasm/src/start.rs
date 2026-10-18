//! Bringing a plugin up: instantiate it, run its `entry` and spawn
//! the panels it declared.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::Granted;
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{Exports, PluginInfo, Status};

use crate::draw::Dirty;
use crate::engine::Host;
use crate::fault::Fault;
use crate::files::FILES;
use crate::host::{Declared, Logged, Offers, Want};
use crate::inbox::Inbox;
use crate::instance::Instance;
use crate::limits::Limits;
use crate::reload::keep;
use crate::stop::{self, Failure};
use crate::storage::DATA;
use crate::{command, panel, run};

/// On the entity of an installed plugin: the file of its component.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Source(pub PathBuf);

/// What a plugin is started with.
pub struct Setting<'a> {
    pub host: &'a Host,
    pub limits: &'a Limits,
    pub agents: &'a AgentList,
    pub papers: &'a Papers,
}

/// A plugin that has run its `entry`.
pub struct Built {
    pub instance: Instance,
    pub panels: Vec<Declared>,
    pub offers: Offers,
    pub wants: Vec<Want>,
    /// What `entry` logged.
    pub logs: Vec<Logged>,
}

/// Instantiates the component in `source`, holding `state` for it,
/// and runs its `entry`.
pub fn build(
    info: &PluginInfo,
    source: &Source,
    granted: &Granted,
    setting: &Setting,
    state: &[u8],
) -> Result<Built, Fault> {
    let Setting {
        host,
        limits,
        agents,
        papers,
    } = setting;
    let instance = Instance::from_file(&host.0, &source.0, limits)?;
    let folder = source.0.parent().unwrap_or(Path::new(""));
    let mut instance = instance
        .named(&info.id)
        .stored(folder.join(DATA), limits)
        .filed(folder.join(FILES), limits)
        .holding(state);
    instance.see(granted, agents, papers);
    let output = instance.entry(limits)?;
    Ok(Built {
        instance,
        panels: output.panels,
        offers: output.offers,
        wants: output.wants,
        logs: output.logs,
    })
}

/// Instantiates the component in `source`, holding `state` for it,
/// and runs its `entry`, for the plugin entity `plugin`. A plugin
/// that does not start is marked failed.
pub fn bring_up(
    plugin: Entity,
    info: &PluginInfo,
    exports: &Exports,
    source: &Source,
    granted: &Granted,
    setting: &Setting,
    state: &[u8],
    commands: &mut Commands,
) {
    let folder = source.0.parent().unwrap_or(Path::new(""));
    match build(info, source, granted, setting, state) {
        Ok(Built {
            instance,
            panels: declared,
            offers,
            wants,
            logs,
        }) => {
            logs.iter()
                .for_each(|line| run::say(commands, &info.id, line));
            run::want(commands, plugin, &wants);
            let shown =
                panel::spawn(plugin, &info.id, &declared, commands);
            command::spawn(
                plugin, &info.id, exports, &offers, commands,
            );
            commands
                .entity(plugin)
                .insert((
                    instance,
                    Status::Running,
                    Inbox::default(),
                    Dirty::of(shown),
                    keep::remember(folder),
                ))
                .remove::<Failure>();
        }
        Err(fault) => {
            commands
                .entity(plugin)
                .insert(keep::trouble(folder, fault.to_string()));
            stop::fail(commands, plugin, &info.id, &fault)
        }
    }
}
