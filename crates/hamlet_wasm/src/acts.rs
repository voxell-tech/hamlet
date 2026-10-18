//! What a plugin does as the chief: a message to an agent and a
//! command run. Each needs a grant the chief gives at its first use,
//! is limited to what the ask lists and to a number a minute, and is
//! noted in the open chat with the plugin's name.

use std::fmt;

use bevy::prelude::*;
use hamlet_sdk::capability::{Capability, Granted, Verdict, Wanted};
use hamlet_sdk::command::{Notice, RunCommand};
use hamlet_sdk::convo;
use hamlet_sdk::plugin::PluginInfo;

use crate::interface::exports::hamlet::plugin::plugin::Intent;
use crate::limits::Limits;
use crate::rate::Rate;
use crate::run::Done;

/// The characters of a message that a notice or a petition quotes.
const QUOTED: usize = 60;

/// The acts each plugin did lately.
#[derive(Resource, Default)]
pub struct Acted(Rate);

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

/// Carries out the acts the plugins that were stepped asked for.
pub fn carry_out(
    mut done: MessageReader<Done>,
    plugins: Query<(&PluginInfo, &Granted)>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    mut acted: ResMut<Acted>,
    mut commands: Commands,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    for Done {
        plugin, intents, ..
    } in done.read()
    {
        let Ok((info, granted)) = plugins.get(*plugin) else {
            continue;
        };
        for intent in intents {
            let (capability, target, text) = match intent {
                Intent::SendChat(message) => (
                    Capability::ChatSend,
                    message.agent.as_str(),
                    message.text.as_str(),
                ),
                Intent::RunAsChief(id) => {
                    (Capability::CommandsRun, id.as_str(), "")
                }
                _ => continue,
            };
            let said = match intent {
                Intent::SendChat(_) => {
                    format!("send to {target}: {}", quoted(text))
                }
                _ => format!("run {target}"),
            };
            if let Err(refused) =
                may(granted, capability, target, text, &limits)
            {
                warn!("plugin {}: {refused}", info.id);
                if refused.ask {
                    commands.trigger(Wanted {
                        plugin: *plugin,
                        capability,
                        target: None,
                        reason: said,
                    });
                }
                continue;
            }
            if !acted.0.admit(*plugin, now, limits.acts_per_minute) {
                warn!(
                    "plugin {}: over {} acts a minute, one dropped",
                    info.id, limits.acts_per_minute
                );
                continue;
            }
            let (id, name) = (info.id.clone(), info.name.clone());
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
                                "{name} ({id}) sent to {}: {}",
                                message.agent,
                                quoted(&message.text)
                            ),
                            Err(why) => {
                                format!(
                                    "{name} ({id}) sent nothing: \
                                     {why}"
                                )
                            }
                        }));
                    });
                }
                Intent::RunAsChief(command) => {
                    info!("plugin {id}: runs {command} as the chief");
                    commands.trigger(Notice(format!(
                        "{name} ({id}) ran {command}"
                    )));
                    commands.trigger(RunCommand::of(command.clone()));
                }
                _ => {}
            }
        }
    }
}

/// Whether the plugin may do the act that needs `capability` for
/// `target`, with `text` to say.
fn may(
    granted: &Granted,
    capability: Capability,
    target: &str,
    text: &str,
    limits: &Limits,
) -> Result<(), Refused> {
    let name = capability.name();
    let scope = granted.scope(capability);
    if !scope.is_empty() && !scope.iter().any(|to| to == target) {
        return Err(Refused::because(format!(
            "{name} is not asked for `{target}`"
        )));
    }
    if text.len() > limits.act_text_bytes {
        return Err(Refused::because(format!(
            "a message has {} bytes, the most is {}",
            text.len(),
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
