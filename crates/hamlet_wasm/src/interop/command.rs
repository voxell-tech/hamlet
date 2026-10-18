//! Running a command of another plugin: the request of the caller,
//! the checks, the command in the target's inbox and the answer back.

use bevy::prelude::*;
use hamlet_sdk::capability::{
    Capability, Granted, Uninstalled, Verdict, Wanted,
};
use hamlet_sdk::plugin::{Manifest, PluginInfo, Status};

use super::calls::Calls;
use super::mail::send;
use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    CallAnswer, CallRequest, CommandCall, Incoming, Intent,
};
use crate::limits::Limits;
use crate::run::Done;

/// What the plugins of a frame may be asked about each other.
type Who<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PluginInfo,
        &'static Manifest,
        &'static Status,
        &'static Granted,
    ),
    Without<Uninstalled>,
>;

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

/// Carries out the `run-command` and `reply` intents of the plugins
/// that were stepped.
pub(super) fn route(
    mut done: MessageReader<Done>,
    who: Who,
    mut inboxes: Query<(&Status, &mut Inbox)>,
    mut calls: ResMut<Calls>,
    limits: Res<Limits>,
    mut commands: Commands,
) {
    for Done { plugin, intents } in done.read() {
        for intent in intents {
            match intent {
                Intent::RunCommand(request) => {
                    run(
                        *plugin,
                        request,
                        &who,
                        &mut inboxes,
                        &mut calls,
                        &limits,
                        &mut commands,
                    );
                }
                Intent::Reply(answer) => reply(
                    *plugin,
                    answer,
                    &who,
                    &mut inboxes,
                    &mut calls,
                    &limits,
                ),
                Intent::LogLine(_) | Intent::Redraw(_) => {}
            }
        }
    }
}

fn run(
    caller: Entity,
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
    match admit(caller, request, who, calls, limits) {
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
    caller: Entity,
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
    let depth = calls.depth_of(caller);
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
            idle(*status)
        )));
    }
    Ok((target, depth))
}

/// What a plugin that is not running is, in a few words.
pub(super) fn idle(status: Status) -> &'static str {
    match status {
        Status::Starting => "not started yet",
        Status::Running => "running",
        Status::Disabled => "switched off",
        Status::Failed => "stopped",
    }
}

fn reply(
    by: Entity,
    answer: &CallAnswer,
    who: &Who,
    inboxes: &mut Query<(&Status, &mut Inbox)>,
    calls: &mut Calls,
    limits: &Limits,
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
    send(
        inboxes,
        call.caller,
        Incoming::Reply(CallAnswer {
            call_id: call.caller_id,
            outcome,
        }),
    );
}
