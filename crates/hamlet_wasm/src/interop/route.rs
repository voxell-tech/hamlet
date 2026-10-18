//! Carrying out what the plugins that were stepped asked of each
//! other.

use bevy::prelude::*;
use hamlet_sdk::plugin::Status;

use super::calls::Calls;
use super::mail::Who;
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
    limits: Res<Limits>,
    mut commands: Commands,
) {
    for Done { plugin, intents } in done.read() {
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
                ),
                Intent::Publish(publication) => publish::publish(
                    *plugin,
                    publication,
                    &who,
                    &mut inboxes,
                    &limits,
                    &mut commands,
                ),
                Intent::LogLine(_) | Intent::Redraw(_) => {}
            }
        }
    }
}
