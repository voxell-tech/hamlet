//! Running a command of another plugin: the request of the caller,
//! the checks, the command in the target's inbox and the answer back.

use bevy::prelude::*;
use hamlet_sdk::capability::{Capability, Verdict, Wanted};
use hamlet_sdk::command::Notice;
use hamlet_sdk::plugin::Status;

use super::calls::{Caller, Calls};
use super::mail::{Who, send};
use super::{invoke, tools};
use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    CallAnswer, CallRequest, CommandCall, Incoming,
};
use crate::limits::Limits;

/// Why a command is not run, and whether the chief is to be asked.
struct Refused {
    why: String,
    ask: bool,
}

impl Refused {
    fn because(why: impl Into<String>) -> Self {
        Self {
            why: why.into(),
            ask: false,
        }
    }
}

/// Starts the command `request` of `caller`, `depth` plugins deep, or
/// answers `caller` with why it is not run.
pub(super) fn run(
    (caller, depth): (Entity, usize),
    request: &CallRequest,
    who: &Who,
    inboxes: &mut Query<(&Status, &mut Inbox)>,
    calls: &mut Calls,
    limits: &Limits,
    commands: &mut Commands,
) {
    let Ok((_, info, ..)) = who.get(caller) else {
        return;
    };
    let target = format!("{}/{}", request.plugin, request.command);
    match admit((caller, depth), request, who, calls, limits) {
        Ok((entity, depth)) => {
            let id = calls.open(
                (caller, request.call_id),
                (entity, &request.plugin),
                depth,
                limits.call_frames,
            );
            info!("plugin {}: runs {target}", info.id);
            send(
                inboxes,
                entity,
                Incoming::Command(CommandCall {
                    call_id: id,
                    sender: info.id.clone(),
                    command: request.command.clone(),
                    args: request.args.clone(),
                }),
            );
        }
        Err(Refused { why, ask }) => {
            warn!("plugin {}: {target} refused: {why}", info.id);
            if ask {
                commands.trigger(Wanted {
                    plugin: caller,
                    capability: Capability::PluginUse,
                    target: Some(request.plugin.clone()),
                    reason: String::new(),
                });
            }
            send(
                inboxes,
                caller,
                Incoming::Reply(CallAnswer {
                    call_id: request.call_id,
                    outcome: Err(why),
                }),
            );
        }
    }
}

/// The plugin that is to run `request` and the depth of the call, or
/// why it is not to be run.
fn admit(
    (caller, depth): (Entity, usize),
    request: &CallRequest,
    who: &Who,
    calls: &Calls,
    limits: &Limits,
) -> Result<(Entity, usize), Refused> {
    let (_, _, manifest, _, granted) =
        who.get(caller).map_err(|_| Refused::because("gone"))?;
    let (plugin, command) = (&request.plugin, &request.command);
    if !manifest.imports.command(plugin, command) {
        return Err(Refused::because(format!(
            "`{plugin}/{command}` is not in the manifest's imports"
        )));
    }
    match granted.verdict_for(Capability::PluginUse, plugin) {
        Some(Verdict::Allowed) => {}
        Some(Verdict::Ask) => {
            return Err(Refused {
                why: "plugin.use not granted".into(),
                ask: true,
            });
        }
        Some(Verdict::Denied) | None => {
            return Err(Refused::because("plugin.use not granted"));
        }
    }
    if request.args.len() > limits.call_bytes {
        return Err(Refused::because(format!(
            "the arguments have {} bytes, the most is {}",
            request.args.len(),
            limits.call_bytes
        )));
    }
    if calls.waiting(caller, request.call_id) {
        return Err(Refused::because(format!(
            "call {} is already waiting",
            request.call_id
        )));
    }
    if calls.in_flight(caller) >= limits.calls_in_flight {
        return Err(Refused::because(format!(
            "{} calls are waiting, the most is {}",
            calls.in_flight(caller),
            limits.calls_in_flight
        )));
    }
    if depth > limits.call_depth {
        return Err(Refused::because(format!(
            "the call would cross {depth} plugins, the most is {}",
            limits.call_depth
        )));
    }
    let (target, _, target_manifest, status, _) = who
        .iter()
        .find(|(_, info, ..)| info.id == *plugin)
        .ok_or_else(|| {
            Refused::because(format!(
                "no plugin `{plugin}` is installed"
            ))
        })?;
    if !target_manifest.exports.commands.contains(command) {
        return Err(Refused::because(format!(
            "`{plugin}` does not export `{command}`"
        )));
    }
    if *status != Status::Running {
        return Err(Refused::because(format!(
            "`{plugin}` is {}",
            status.words()
        )));
    }
    Ok((target, depth))
}

/// Gives the caller the answer `by` made to a command.
pub(super) fn reply(
    by: Entity,
    answer: &CallAnswer,
    who: &Who,
    inboxes: &mut Query<(&Status, &mut Inbox)>,
    calls: &mut Calls,
    limits: &Limits,
    commands: &mut Commands,
) {
    let name = who.get(by).map_or("", |(_, info, ..)| &info.id);
    let Some(call) = calls.answer(answer.call_id, by) else {
        warn!(
            "plugin {name}: replied to call {}, which is not \
             waiting for it",
            answer.call_id
        );
        return;
    };
    if let Caller::Invoker { entity, id, result } = &call.caller {
        let outcome = invoke::checked(
            result,
            &answer.outcome,
            &call.target_id,
            limits,
        );
        invoke::answer(inboxes, (*entity, *id), outcome);
        return;
    }
    let outcome = match &answer.outcome {
        Ok(bytes) if bytes.len() > limits.call_bytes => Err(format!(
            "the reply has {} bytes, the most is {}",
            bytes.len(),
            limits.call_bytes
        )),
        Err(why) => Err(why
            [..why.floor_char_boundary(limits.text_bytes)]
            .to_string()),
        other => other.clone(),
    };
    match call.caller {
        Caller::Plugin { entity, id } => {
            send(
                inboxes,
                entity,
                Incoming::Reply(CallAnswer {
                    call_id: id,
                    outcome,
                }),
            );
        }
        Caller::Invoker { .. } => {}
        Caller::Agent => calls.tell_agent(
            call.id,
            outcome.map(|bytes| tools::text(&bytes, limits)),
        ),
        Caller::Chief(command) => {
            if let Err(why) = outcome {
                let words = format!(
                    "`{}/{command}` failed: {why}",
                    call.target_id
                );
                warn!("{words}");
                commands.trigger(Notice(words));
            }
        }
    }
}
