//! Stepping the plugins that have mail, and carrying out what they
//! ask.

use bevy::prelude::*;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{Granted, Wanted};
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{Manifest, PluginInfo, Status};
use hamlet_sdk::tree::id::dock_id;

use crate::draw::Dirty;
use crate::host::{Logged, Want};
use crate::inbox::Inbox;
use crate::instance::Instance;
use crate::interface::exports::hamlet::plugin::plugin::Intent;
use crate::interface::hamlet::plugin::host::Level;
use crate::limits::Limits;
use crate::reach::{self, Sharing};
use crate::{event, stop};

/// What one plugin's `step` asked for.
#[derive(Message)]
pub struct Done {
    pub plugin: Entity,
    pub intents: Vec<Intent>,
    /// The collections that changed in the call, each as the plugin
    /// that shares it and its name.
    pub changes: Vec<(String, String)>,
}

/// Calls `step` on each running plugin that has mail, once. A plugin
/// whose call fails is marked failed.
pub fn step(
    limits: Res<Limits>,
    agents: Res<AgentList>,
    papers: Res<Papers>,
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &Manifest,
        &Status,
        &Granted,
        &mut Inbox,
        &mut Instance,
        &mut Dirty,
    )>,
    sharing: Sharing,
    mut done: MessageWriter<Done>,
    mut commands: Commands,
) {
    for (
        plugin,
        info,
        manifest,
        status,
        granted,
        mut inbox,
        mut instance,
        mut dirty,
    ) in &mut plugins
    {
        if *status != Status::Running || inbox.is_empty() {
            continue;
        }
        instance.see(granted, &agents, &papers);
        instance.reach(reach::of(manifest, &sharing, &limits));
        let mail = inbox.take();
        match instance.step(&limits, &mail) {
            Ok(output) => {
                output.logs.iter().for_each(|line| {
                    say(&mut commands, &info.id, line)
                });
                want(&mut commands, plugin, &output.wants);
                // A panel that was written to may look different now.
                for local in mail.iter().filter_map(event::panel_of) {
                    dirty.mark(&dock_id(&info.id, local));
                }
                done.write(Done {
                    plugin,
                    intents: output.intents,
                    changes: output.changes,
                });
            }
            Err(fault) => {
                stop::fail(&mut commands, plugin, &info.id, &fault)
            }
        }
    }
}

/// Carries out the intents of the plugins that were stepped.
pub fn apply(
    mut done: MessageReader<Done>,
    mut plugins: Query<(&PluginInfo, Option<&mut Dirty>)>,
    mut said: MessageWriter<Said>,
) {
    for Done {
        plugin, intents, ..
    } in done.read()
    {
        let Ok((info, mut dirty)) = plugins.get_mut(*plugin) else {
            continue;
        };
        for intent in intents {
            match (intent, dirty.as_mut()) {
                (Intent::LogLine(text), _) => {
                    info!("plugin {}: {text}", info.id);
                    said.write(Said {
                        plugin: info.id.clone(),
                        level: Level::Info,
                        text: text.clone(),
                    });
                }
                (Intent::Redraw(panel), Some(dirty)) => {
                    dirty.mark(&dock_id(&info.id, panel))
                }
                (Intent::Redraw(_), None) => {}
                // Between plugins: `interop` carries these out.
                (
                    Intent::RunCommand(_)
                    | Intent::Reply(_)
                    | Intent::Publish(_),
                    _,
                ) => {}
            }
        }
    }
}

/// Tells the SDK what the plugin `plugin` tried to use that the chief
/// is yet to decide, so that the chief is asked.
pub fn want(commands: &mut Commands, plugin: Entity, wants: &[Want]) {
    for want in wants {
        commands.trigger(Wanted {
            plugin,
            capability: want.capability,
            target: want.target.clone(),
            reason: want.reason.clone(),
        });
    }
}

/// A line a plugin logged, as it was written to the app's log.
#[derive(Message, Clone, PartialEq, Debug)]
pub struct Said {
    pub plugin: String,
    pub level: Level,
    pub text: String,
}

/// Writes a line a plugin logged to the app's log, labelled with the
/// plugin, and tells of it with [`Said`].
pub fn say(commands: &mut Commands, id: &str, line: &Logged) {
    let text = &line.message;
    match line.level {
        Level::Info => info!("plugin {id}: {text}"),
        Level::Warn => warn!("plugin {id}: {text}"),
        Level::Error => error!("plugin {id}: {text}"),
    }
    commands.write_message(Said {
        plugin: id.into(),
        level: line.level,
        text: text.clone(),
    });
}
