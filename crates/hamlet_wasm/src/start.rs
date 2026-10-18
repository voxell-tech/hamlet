//! Bringing a plugin up: instantiate it, run its `entry` and spawn
//! the panels it declared.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use hamlet_sdk::plugin::{PluginInfo, Status};

use crate::draw::Dirty;
use crate::engine::Host;
use crate::inbox::Inbox;
use crate::instance::Instance;
use crate::limits::Limits;
use crate::{panel, run};

/// On the entity of an installed plugin: the file of its component.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Source(pub PathBuf);

/// Instantiates the component in `wasm` and runs its `entry`, for
/// the plugin entity `plugin`. A plugin that does not start is
/// marked failed.
pub fn bring_up(
    plugin: Entity,
    info: &PluginInfo,
    wasm: &Path,
    host: &Host,
    limits: &Limits,
    commands: &mut Commands,
) {
    let started = Instance::from_file(&host.0, wasm, limits)
        .and_then(|mut instance| {
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
            commands.entity(plugin).insert((
                instance,
                Status::Running,
                Inbox::default(),
                Dirty::of(shown),
            ));
        }
        Err(fault) => {
            warn!("plugin {}: {fault}", info.id);
            commands.entity(plugin).insert(Status::Failed);
        }
    }
}
