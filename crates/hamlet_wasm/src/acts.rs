//! What a plugin does as the chief: a message to an agent, a command
//! run and a click on a thing of the world. Each needs a grant the
//! chief gives at its first use, is limited to what the ask lists and
//! to a number a minute, and is noted in the open chat with the
//! plugin's name.

use std::fmt;

use bevy::prelude::*;
use hamlet_agents_api::convo;
use hamlet_realm_api::pick::{Gesture, Named, PointAt};
use hamlet_realm_api::survey::Survey;
use hamlet_sdk::capability::{Capability, Granted, Verdict, Wanted};
use hamlet_sdk::command::{Notice, RunCommand};
use hamlet_sdk::plugin::PluginInfo;

use crate::interface::exports::hamlet::plugin::plugin::{
    Intent, WorldOrder, WorldTarget,
};
use crate::limits::Limits;
use crate::rate::Rate;
use crate::run::Done;

/// The characters of a message that a notice or a petition quotes.
const QUOTED: usize = 60;

/// The acts each plugin did lately.
#[derive(Resource, Default)]
pub struct Acted {
    done: Rate,
    /// A warning is logged once a minute for each plugin and kind.
    refused: Rate,
    dropped: Rate,
}

/// Why an act is not done, and whether the chief is to be asked.
struct Refused {
    why: String,
    ask: bool,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.why)
    }
}

impl Refused {
    fn because(why: impl Into<String>) -> Self {
        Self {
            why: why.into(),
            ask: false,
        }
    }
}

/// One act a plugin asked for: what it needs, what it reaches, the
/// text it says and the words a petition or a notice tells it by.
struct Act {
    capability: Capability,
    target: String,
    text: String,
    said: String,
}

/// Carries out the acts the plugins that were stepped asked for. An
/// act of a plugin answering the calls of others needs the grant of
/// each of them too, so that no plugin acts as the chief through
/// another, and is noted with their names.
pub fn carry_out(
    mut done: MessageReader<Done>,
    plugins: Query<(&PluginInfo, &Granted)>,
    survey: Res<Survey>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    mut acted: ResMut<Acted>,
    mut commands: Commands,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    for Done {
        plugin,
        intents,
        callers,
        ..
    } in done.read()
    {
        let Ok((info, granted)) = plugins.get(*plugin) else {
            continue;
        };
        let callers = callers
            .iter()
            .map(|caller| (*caller, plugins.get(*caller).ok()))
            .collect::<Vec<_>>();
        for intent in intents {
            let act = match intent {
                Intent::SendChat(message) => Ok(Act {
                    capability: Capability::ChatSend,
                    target: message.agent.clone(),
                    text: message.text.clone(),
                    said: format!(
                        "send to {}: {}",
                        message.agent,
                        quoted(&message.text)
                    ),
                }),
                Intent::RunAsChief(id) => Ok(Act {
                    capability: Capability::CommandsRun,
                    target: id.clone(),
                    text: String::new(),
                    said: format!("run {id}"),
                }),
                Intent::WorldAct(order) => pointing(order, &survey),
                _ => continue,
            };
            let checked = act.map_err(Refused::because).and_then(
                |act| match may(granted, &act, &limits) {
                    Ok(()) => Ok(act),
                    Err(refused) => {
                        if refused.ask {
                            commands.trigger(Wanted {
                                plugin: *plugin,
                                capability: act.capability,
                                target: None,
                                reason: act.said,
                            });
                        }
                        Err(refused)
                    }
                },
            );
            let checked = checked.and_then(|act| {
                for (caller, found) in &callers {
                    let Some((by, granted)) = found else {
                        return Err(Refused::because(
                            "it answers a plugin that is gone",
                        ));
                    };
                    if let Err(refused) = may(granted, &act, &limits)
                    {
                        if refused.ask {
                            commands.trigger(Wanted {
                                plugin: *caller,
                                capability: act.capability,
                                target: None,
                                reason: act.said.clone(),
                            });
                        }
                        return Err(Refused::because(format!(
                            "it answers {}, which may not: {refused}",
                            by.id
                        )));
                    }
                }
                Ok(act)
            });
            let act = match checked {
                Ok(act) => act,
                Err(refused) => {
                    if acted.refused.admit(*plugin, now, 1) {
                        warn!("plugin {}: {refused}", info.id);
                    }
                    continue;
                }
            };
            if !acted.done.admit(*plugin, now, limits.acts_per_minute)
            {
                if acted.dropped.admit(*plugin, now, 1) {
                    warn!(
                        "plugin {}: over {} acts a minute, some \
                         dropped",
                        info.id, limits.acts_per_minute
                    );
                }
                continue;
            }
            let by = callers
                .iter()
                .filter_map(|(_, found)| {
                    found.map(|(by, _)| by.id.as_str())
                })
                .collect::<Vec<_>>()
                .join(", ");
            // The plugin, and those whose calls it answers.
            let (id, who) = match by.is_empty() {
                true => (
                    info.id.clone(),
                    format!("{} ({})", info.name, info.id),
                ),
                false => (
                    format!("{}, called by {by}", info.id),
                    format!(
                        "{} ({}), called by {by},",
                        info.name, info.id
                    ),
                ),
            };
            match intent {
                Intent::SendChat(message) => {
                    let message = message.clone();
                    commands.queue(move |world: &mut World| {
                        let sent = convo::send_as_chief(
                            world,
                            &message.agent,
                            &id,
                            &message.text,
                        );
                        world.trigger(Notice(match sent {
                            Ok(()) => format!(
                                "{who} sent to {}: {}",
                                message.agent,
                                quoted(&message.text)
                            ),
                            Err(why) => {
                                format!("{who} sent nothing: {why}")
                            }
                        }));
                    });
                }
                Intent::RunAsChief(command) => {
                    info!("plugin {id}: runs {command} as the chief");
                    commands.trigger(Notice(format!(
                        "{who} ran {command}"
                    )));
                    commands.trigger(RunCommand::of(command.clone()));
                }
                Intent::WorldAct(order) => {
                    info!("plugin {id}: {} as the chief", act.said);
                    commands.trigger(Notice(format!(
                        "{who} {}",
                        done_words(order)
                    )));
                    commands.trigger(point_at(order));
                }
                _ => {}
            }
        }
    }
}

/// The act of a click on a thing of the world, whose target is the
/// hamlet the thing is of, or why there is none.
fn pointing(
    order: &WorldOrder,
    survey: &Survey,
) -> Result<Act, String> {
    let (gesture, target) = parts(order);
    let hamlet = match target {
        WorldTarget::Agent(name) => survey
            .agents
            .iter()
            .find(|agent| agent.name == *name)
            .map(|agent| agent.hamlet.clone())
            .ok_or_else(|| {
                format!("no agent `{name}` in the world")
            })?,
        WorldTarget::Hamlet(id) => survey
            .hamlets
            .iter()
            .find(|hamlet| hamlet.id == *id)
            .map(|hamlet| hamlet.id.clone())
            .ok_or_else(|| {
                format!("no hamlet `{id}` in the world")
            })?,
    };
    Ok(Act {
        capability: Capability::WorldAct,
        target: hamlet,
        text: String::new(),
        said: format!("{} {}", verb(gesture), named(target)),
    })
}

fn parts(order: &WorldOrder) -> (Gesture, &WorldTarget) {
    match order {
        WorldOrder::Focus(target) => (Gesture::Focus, target),
        WorldOrder::Follow(target) => (Gesture::Follow, target),
        WorldOrder::Select(target) => (Gesture::Select, target),
    }
}

fn named(target: &WorldTarget) -> &str {
    match target {
        WorldTarget::Agent(name) | WorldTarget::Hamlet(name) => name,
    }
}

fn verb(gesture: Gesture) -> &'static str {
    match gesture {
        Gesture::Focus => "focus on",
        Gesture::Follow => "follow",
        Gesture::Select => "select",
    }
}

/// What a notice says the plugin did.
fn done_words(order: &WorldOrder) -> String {
    let (gesture, target) = parts(order);
    let done = match gesture {
        Gesture::Focus => "focused on",
        Gesture::Follow => "followed",
        Gesture::Select => "selected",
    };
    format!("{done} {}", named(target))
}

/// The realm's event for `order`.
fn point_at(order: &WorldOrder) -> PointAt {
    let (gesture, target) = parts(order);
    let named = match target {
        WorldTarget::Agent(name) => Named::Agent(name.clone()),
        WorldTarget::Hamlet(id) => Named::Hamlet(id.clone()),
    };
    PointAt { named, gesture }
}

/// Whether the plugin may do `act`.
fn may(
    granted: &Granted,
    act: &Act,
    limits: &Limits,
) -> Result<(), Refused> {
    let (capability, target) = (act.capability, &act.target);
    let name = capability.name();
    let scope = granted.scope(capability);
    if !scope.iter().any(|to| to == target) {
        return Err(Refused::because(format!(
            "{name} is not asked for `{target}`"
        )));
    }
    if act.text.len() > limits.act_text_bytes {
        return Err(Refused::because(format!(
            "a message has {} bytes, the most is {}",
            act.text.len(),
            limits.act_text_bytes
        )));
    }
    match granted.verdict(capability) {
        Some(Verdict::Allowed) => Ok(()),
        Some(Verdict::Ask) => Err(Refused {
            why: format!("{name} is yet to be decided"),
            ask: true,
        }),
        Some(Verdict::Denied) | None => {
            Err(Refused::because(format!("{name} not granted")))
        }
    }
}

fn quoted(text: &str) -> String {
    text.chars().take(QUOTED).collect()
}
