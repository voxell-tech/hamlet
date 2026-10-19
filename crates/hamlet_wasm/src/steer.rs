//! A point of an agent's turn put to the plugins that steer the
//! agents of its hamlet: each is stepped at once, under a budget of
//! time and fuel of its own, while the turn waits, and may add a
//! note or hold a tool call back. A plugin that fails or runs out
//! changes nothing.

use bevy::prelude::*;
use hamlet_agents_api::tools::{Steer, Steered, Steering};
use hamlet_realm_api::survey::Survey;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{Capability, Granted, Verdict, Wanted};
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{Manifest, PluginInfo, Status};

use crate::instance::Instance;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Intent, SteerChange, SteerPoint, SteerTool,
};
use crate::interop::Calls;
use crate::limits::Limits;
use crate::reach::{self, Sharing};
use crate::run::{self, Done};
use crate::stop;

/// The number of the last point of a turn put to a plugin.
#[derive(Resource, Default)]
pub struct Points(u64);

/// Puts the point of a turn to each running plugin that holds
/// `agent.steer` for the agent's hamlet, and adds what each made of
/// it. A plugin the chief is yet to decide for is asked, and is not
/// stepped.
pub fn steer(
    mut steering: On<Steering>,
    limits: Res<Limits>,
    agents: Res<AgentList>,
    papers: Res<Papers>,
    survey: Res<Survey>,
    mut points: ResMut<Points>,
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &Manifest,
        &Status,
        &Granted,
        &mut Instance,
    )>,
    sharing: Sharing,
    calls: Res<Calls>,
    mut done: MessageWriter<Done>,
    mut commands: Commands,
) {
    let point = steering.event_mut();
    let budget = limits.steering();
    for (plugin, info, manifest, status, granted, mut instance) in
        &mut plugins
    {
        let listed = granted.scope(Capability::AgentSteer);
        if *status != Status::Running
            || (!listed.is_empty() && !listed.contains(&point.hamlet))
        {
            continue;
        }
        match granted.verdict(Capability::AgentSteer) {
            Some(Verdict::Allowed) => {}
            Some(Verdict::Ask) => {
                commands.trigger(Wanted {
                    plugin,
                    capability: Capability::AgentSteer,
                    target: None,
                    reason: String::new(),
                });
                continue;
            }
            Some(Verdict::Denied) | None => continue,
        }
        point.watched = true;
        points.0 += 1;
        let id = points.0;
        instance.see(granted, &agents, &papers);
        instance.see_world(granted, &survey);
        instance.reach(reach::of(manifest, &sharing, &limits));
        let given = Incoming::Steer(SteerPoint {
            id,
            agent: point.agent.clone(),
            hamlet: point.hamlet.clone(),
            step: point.step,
            tool: point.tool.as_ref().map(|(name, args)| SteerTool {
                name: name.clone(),
                args: cut(args, limits.steer_args_bytes),
            }),
        });
        let steered = match instance.step(&budget, &[given]) {
            Ok(mut output) => {
                output.logs.iter().for_each(|line| {
                    run::say(&mut commands, &info.id, line)
                });
                run::want(&mut commands, plugin, &output.wants);
                let answer = answered(&mut output.intents, id);
                done.write(Done {
                    plugin,
                    intents: output.intents,
                    changes: output.changes,
                    callers: calls.callers(plugin),
                });
                answer.and_then(|change| {
                    made(change, point.tool.is_some(), &limits)
                })
            }
            Err(fault) => {
                stop::fail(&mut commands, plugin, &info.id, &fault);
                Some(Steered::Failed(fault.to_string()))
            }
        };
        if let Some(steered) = steered {
            if !matches!(steered, Steered::Failed(_)) {
                info!(
                    "plugin {} steers {} of {}: {steered:?}",
                    info.id, point.agent, point.hamlet
                );
            }
            point.steers.push(Steer {
                plugin: info.id.clone(),
                steered,
            });
        }
    }
}

/// Takes the plugin's answer to the point `id` out of `intents`, if
/// it gave one.
fn answered(
    intents: &mut Vec<Intent>,
    id: u64,
) -> Option<SteerChange> {
    let at = intents.iter().position(|intent| {
        matches!(intent, Intent::SteerAnswer(reply) if reply.id == id)
    })?;
    match intents.remove(at) {
        Intent::SteerAnswer(reply) => Some(reply.change),
        _ => None,
    }
}

/// What a plugin's answer makes of a point: a note before a model
/// call, a hold before a tool call, each as one plain line cut to its
/// cap. An answer of the other kind changes nothing.
fn made(
    change: SteerChange,
    tool: bool,
    limits: &Limits,
) -> Option<Steered> {
    let words = |text: &str| {
        cut(&hamlet_base::plain(text), limits.steer_note_bytes)
    };
    match (change, tool) {
        (SteerChange::Note(text), false)
            if !text.trim().is_empty() =>
        {
            Some(Steered::Note(words(&text)))
        }
        (SteerChange::Hold(why), true) => {
            Some(Steered::Hold(words(&why)))
        }
        _ => None,
    }
}

/// `text` in at most `most` bytes.
fn cut(text: &str, most: usize) -> String {
    text[..text.floor_char_boundary(most)].to_string()
}
