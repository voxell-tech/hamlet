//! Stepping the plugins that have mail, and carrying out what they
//! ask.

use bevy::prelude::*;
use hamlet_sdk::plugin::{PluginInfo, Status};

use crate::draw::Dirty;
use crate::host::Logged;
use crate::inbox::Inbox;
use crate::instance::Instance;
use crate::interface::exports::hamlet::plugin::plugin::Intent;
use crate::interface::hamlet::plugin::host::Level;
use crate::limits::Limits;
use crate::panel;

/// What one plugin's `step` asked for.
#[derive(Message)]
pub struct Done {
    pub plugin: Entity,
    pub intents: Vec<Intent>,
}

/// Calls `step` on each running plugin that has mail, once. A plugin
/// whose call fails is marked failed.
pub fn step(
    limits: Res<Limits>,
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &Status,
        &mut Inbox,
        &mut Instance,
    )>,
    mut done: MessageWriter<Done>,
    mut commands: Commands,
) {
    for (plugin, info, status, mut inbox, mut instance) in
        &mut plugins
    {
        if *status != Status::Running || inbox.is_empty() {
            continue;
        }
        match instance.step(&limits, &inbox.take()) {
            Ok(output) => {
                output
                    .logs
                    .iter()
                    .for_each(|line| say(&info.id, line));
                done.write(Done {
                    plugin,
                    intents: output.intents,
                });
            }
            Err(fault) => {
                warn!("plugin {}: {fault}", info.id);
                commands.entity(plugin).insert(Status::Failed);
            }
        }
    }
}

/// Carries out the intents of the plugins that were stepped.
pub fn apply(
    mut done: MessageReader<Done>,
    mut plugins: Query<(&PluginInfo, Option<&mut Dirty>)>,
) {
    for Done { plugin, intents } in done.read() {
        let Ok((info, mut dirty)) = plugins.get_mut(*plugin) else {
            continue;
        };
        for intent in intents {
            match (intent, dirty.as_mut()) {
                (Intent::LogLine(text), _) => {
                    info!("plugin {}: {text}", info.id)
                }
                (Intent::Redraw(panel), Some(dirty)) => {
                    dirty.mark(&panel::dock_id(&info.id, panel))
                }
                (Intent::Redraw(_), None) => {}
            }
        }
    }
}

/// Writes a line a plugin logged to the app's log, labelled with the
/// plugin.
pub fn say(id: &str, line: &Logged) {
    let text = &line.message;
    match line.level {
        Level::Info => info!("plugin {id}: {text}"),
        Level::Warn => warn!("plugin {id}: {text}"),
        Level::Error => error!("plugin {id}: {text}"),
    }
}
