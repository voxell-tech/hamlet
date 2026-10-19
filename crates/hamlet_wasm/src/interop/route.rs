//! Carrying out what the plugins that were stepped asked of each
//! other.

use bevy::prelude::*;
use hamlet_sdk::plugin::Status;

use super::calls::Calls;
use super::mail::Who;
use super::publish::Flow;
use super::{command, publish};
use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::Intent;
use crate::limits::Limits;
use crate::run::Done;

/// Carries out the `run-command`, `reply` and `publish` intents of
/// the plugins that were stepped.
pub(super) fn route(
    mut done: MessageReader<Done>,
    who: Who,
    mut inboxes: Query<(&Status, &mut Inbox)>,
    mut calls: ResMut<Calls>,
    mut flow: ResMut<Flow>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    mut commands: Commands,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    for Done {
        plugin,
        intents,
        changes,
    } in done.read()
    {
        let mut spent = 0;
        for (owner, name) in changes {
            publish::changed(
                owner,
                name,
                &who,
                &mut inboxes,
                &mut commands,
            );
        }
        for intent in intents {
            match intent {
                Intent::RunCommand(request) => command::run(
                    *plugin,
                    request,
                    &who,
                    &mut inboxes,
                    &mut calls,
                    &limits,
                    &mut commands,
                ),
                Intent::Reply(answer) => command::reply(
                    *plugin,
                    answer,
                    &who,
                    &mut inboxes,
                    &mut calls,
                    &limits,
                    &mut commands,
                ),
                Intent::Publish(publication) => {
                    let more = publication.payload.len();
                    let id = who
                        .get(*plugin)
                        .map_or("", |(_, info, ..)| info.id.as_str());
                    if flow.admit(
                        *plugin,
                        id,
                        (spent, more),
                        now,
                        &limits,
                    ) {
                        spent += more;
                        publish::publish(
                            *plugin,
                            publication,
                            &who,
                            &mut inboxes,
                            &limits,
                            &mut commands,
                        );
                    }
                }
                Intent::LogLine(_)
                | Intent::Redraw(_)
                | Intent::OpenPanel(_)
                | Intent::SendChat(_)
                | Intent::RunAsChief(_)
                | Intent::WorldAct(_)
                | Intent::Overlay(_)
                | Intent::PickFile(_)
                | Intent::Fetch(_) => {}
            }
        }
    }
}
