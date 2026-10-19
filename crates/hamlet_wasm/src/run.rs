//! Stepping the plugins that have mail, and carrying out what they
//! ask.

use bevy::prelude::*;
use hamlet_realm_api::survey::Survey;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{Granted, Wanted};
use hamlet_sdk::command::Notice;
use hamlet_sdk::dock::{self, Panel};
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{
    ContributedBy, Manifest, PluginInfo, Status,
};
use hamlet_sdk::tree::id::dock_id;

use crate::draw::Dirty;
use crate::host::{Logged, Want};
use crate::inbox::Inbox;
use crate::instance::Instance;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Intent,
};
use crate::interface::hamlet::plugin::host::Level;
use crate::interop::Calls;
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
    /// The plugins whose calls the plugin was answering when it was
    /// stepped. An act as the chief needs their grant too.
    pub callers: Vec<Entity>,
}

/// Calls `step` on each running plugin that has mail, once. A plugin
/// whose call fails is marked failed, and where all its mail was one
/// other plugin's commands and events, that plugin may not reach it
/// until the chief starts it again.
pub fn step(
    limits: Res<Limits>,
    agents: Res<AgentList>,
    papers: Res<Papers>,
    survey: Res<Survey>,
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
    named: Query<(Entity, &PluginInfo)>,
    mut calls: ResMut<Calls>,
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
        instance.see_world(granted, &survey);
        instance.reach(reach::of(manifest, &sharing, &limits));
        let mail = inbox.take();
        let callers = calls.callers(plugin);
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
                    callers,
                });
            }
            Err(fault) => {
                if let Some((sender, by)) =
                    sender_of(&mail, plugin, &calls, &named)
                {
                    calls.bar(sender, plugin);
                    commands.trigger(Notice(format!(
                        "A command or an event of {by} stopped {}: \
                         {fault}. {by} may not reach it again until \
                         you restart {}.",
                        info.name, info.name
                    )));
                }
                stop::fail(&mut commands, plugin, &info.id, &fault)
            }
        }
    }
}

/// The one plugin, and its name, whose commands and events were all
/// of `mail`, if there was one.
fn sender_of<'a>(
    mail: &[Incoming],
    plugin: Entity,
    calls: &Calls,
    named: &'a Query<(Entity, &PluginInfo)>,
) -> Option<(Entity, &'a str)> {
    let mut senders = mail.iter().map(|incoming| match incoming {
        Incoming::Command(call) => {
            calls.commander(call.call_id, plugin)
        }
        Incoming::Event(event) => named
            .iter()
            .find(|(_, info)| info.id == event.sender)
            .map(|(sender, _)| sender),
        _ => None,
    });
    let first = senders.next()??;
    if !senders.all(|other| other == Some(first)) {
        return None;
    }
    let (_, info) = named.get(first).ok()?;
    Some((first, &info.name))
}

/// Carries out the intents of the plugins that were stepped.
pub fn apply(
    mut done: MessageReader<Done>,
    mut plugins: Query<(&PluginInfo, Option<&mut Dirty>)>,
    panels: Query<(&Panel, &ContributedBy)>,
    mut said: MessageWriter<Said>,
    mut commands: Commands,
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
                (Intent::OpenPanel(panel), _) => {
                    let id = dock_id(&info.id, panel);
                    let own = panels.iter().any(|(held, by)| {
                        by.0 == *plugin && held.id == id
                    });
                    if own {
                        commands.queue(move |world: &mut World| {
                            dock::open(world, &id)
                        });
                    } else {
                        warn!(
                            "plugin {}: no panel `{panel}` to open",
                            info.id
                        );
                    }
                }
                // Between plugins: `interop` carries these out.
                // As the chief: `acts` does. Over the web: `net`. In
                // a folder: `pick` and `folder`. To the model:
                // `model`.
                (
                    Intent::RunCommand(_)
                    | Intent::Invoke(_)
                    | Intent::Reply(_)
                    | Intent::Publish(_)
                    | Intent::SendChat(_)
                    | Intent::RunAsChief(_)
                    | Intent::WorldAct(_)
                    | Intent::Overlay(_)
                    | Intent::PickFile(_)
                    | Intent::PickFolder(_)
                    | Intent::Folder(_)
                    | Intent::ModelCall(_)
                    | Intent::Fetch(_),
                    _,
                ) => {}
                // Taken where the point was put, by `steer`; one
                // given in any other `step` answers nothing.
                (Intent::SteerAnswer(_), _) => {}
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
