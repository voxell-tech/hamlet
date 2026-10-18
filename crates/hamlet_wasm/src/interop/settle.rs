//! Answering the calls whose target will not: it was switched off,
//! stopped or removed, or it did not answer in time.

use bevy::prelude::*;
use hamlet_sdk::command::Notice;
use hamlet_sdk::plugin::{PluginInfo, Status};

use super::calls::{Call, Caller, Calls};
use super::mail::send;
use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    CallAnswer, Incoming,
};
use crate::limits::Limits;

/// How a call that is not answered ends.
enum End {
    /// Nobody is left to tell.
    Quietly,
    /// The caller is told why.
    Told(String),
}

/// Counts a frame, and ends each call whose target is not running
/// or is out of time, with an error for its caller.
pub(super) fn settle(
    mut calls: ResMut<Calls>,
    plugins: Query<(&PluginInfo, &Status)>,
    mut inboxes: Query<(&Status, &mut Inbox)>,
    limits: Res<Limits>,
    mut commands: Commands,
) {
    calls.frame += 1;
    let frame = calls.frame;
    let ended = calls
        .open
        .iter()
        .filter_map(|call| {
            Some((call.id, ending(call, frame, &plugins, &limits)?))
        })
        .collect::<Vec<_>>();
    for (id, end) in ended {
        let Some(call) = calls.close(id) else {
            continue;
        };
        let End::Told(why) = end else {
            continue;
        };
        match &call.caller {
            Caller::Plugin { entity, id } => {
                warn!(
                    "plugin {}: the call to {} ended: {why}",
                    caller_name(*entity, &plugins),
                    call.target_id
                );
                send(
                    &mut inboxes,
                    *entity,
                    Incoming::Reply(CallAnswer {
                        call_id: *id,
                        outcome: Err(why),
                    }),
                );
            }
            Caller::Chief(command) => {
                let words = format!(
                    "`{}/{command}` did not run: {why}",
                    call.target_id
                );
                warn!("{words}");
                commands.trigger(Notice(words));
            }
        }
    }
}

fn caller_name(
    caller: Entity,
    plugins: &Query<(&PluginInfo, &Status)>,
) -> String {
    plugins
        .get(caller)
        .map(|(info, _)| info.id.clone())
        .unwrap_or_default()
}

fn ending(
    call: &Call,
    frame: u64,
    plugins: &Query<(&PluginInfo, &Status)>,
    limits: &Limits,
) -> Option<End> {
    if let Caller::Plugin { entity, .. } = call.caller
        && !matches!(plugins.get(entity), Ok((_, Status::Running)))
    {
        return Some(End::Quietly);
    }
    let target = &call.target_id;
    match plugins.get(call.target) {
        Err(_) => Some(End::Told(format!("`{target}` was removed"))),
        Ok((_, status)) if *status != Status::Running => Some(
            End::Told(format!("`{target}` is {}", status.words())),
        ),
        Ok(_) if frame >= call.due => Some(End::Told(format!(
            "`{target}` did not answer in {} frames",
            limits.call_frames
        ))),
        Ok(_) => None,
    }
}
