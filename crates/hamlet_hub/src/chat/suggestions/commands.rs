//! The commands the field lists after `/`, each run as its control
//! does when the line is sent. Only those about the agent the chat is
//! with are here: the rest have a menu of their own.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use hamlet_core::client::messages::ChiefMessage;
use hamlet_core::client::realm::{ActionUsage, Actions};
use hamlet_core::protocol::client::RequestId;
use hamlet_core::protocol::signs::Sign;

use super::super::{asked_in, replied};
use super::{Entry, Pick, Source};
use crate::convo::agent_of;
use crate::hub::thread::compact_now;
use crate::world_menu;

/// Each command listed, with what runs it for a Hub's chat and says
/// whether it went out.
const LISTED: [(&str, fn(&mut World, Entity) -> bool); 5] = [
    ("compact", |world, hub| {
        let agent = agent_of(world, hub);
        let request = compact_now(world, agent);
        made_in(world, hub, request)
    }),
    ("retry", |world, hub| say(world, hub, "/retry")),
    ("stop", |world, hub| {
        let agent = agent_of(world, hub);
        let request = world_menu::stop(world, agent);
        replied(world, hub, request);
        true
    }),
    ("reflect", |world, hub| say(world, hub, "/reflect")),
    ("model", |world, hub| {
        let agent = agent_of(world, hub);
        world_menu::model::open(world, agent);
        true
    }),
];

/// Says the answer to `request`, if it was made, under `hub`'s field,
/// and whether it was made.
fn made_in(
    world: &mut World,
    hub: Entity,
    request: Option<RequestId>,
) -> bool {
    let Some(request) = request else {
        return false;
    };
    asked_in(world, request, hub);
    true
}

pub(super) struct Commands;

impl Source for Commands {
    const SIGN: Sign = Sign::Command;
    const LISTS: &str = "commands";

    fn entries(world: &World, _: Entity, rest: &str) -> Vec<Entry> {
        let rest = rest.to_lowercase();
        let usage = world.get_resource::<ActionUsage>();
        LISTED
            .into_iter()
            .filter(|(name, _)| {
                name.starts_with(&rest)
                    && Actions::is_allowed(world, name)
            })
            .map(|(name, _)| Entry {
                title: format!("/{name}"),
                detail: usage
                    .and_then(|usage| {
                        usage
                            .0
                            .iter()
                            .find(|action| action.name == name)
                    })
                    .map(|action| action.description.clone())
                    .unwrap_or_default(),
                pick: Pick::Words(format!("/{name} ")),
            })
            .collect()
    }
}

/// Runs the command `line` is for `hub`'s chat, when it is one listed
/// and allowed. None when it is not one; else whether it went out,
/// which a command to an agent with no address does not.
pub(in crate::chat) fn run(
    world: &mut World,
    hub: Entity,
    line: &str,
) -> Option<bool> {
    let name = line.strip_prefix(Sign::Command.char())?;
    let (_, run) = LISTED.into_iter().find(|(listed, _)| {
        *listed == name && Actions::is_allowed(world, listed)
    })?;
    Some(run(world, hub))
}

/// Sends the command `line` to the agent `hub`'s chat is with, and
/// says whether it went.
fn say(world: &mut World, hub: Entity, line: &str) -> bool {
    let Some(to) = crate::convo::of(world, hub)
        .and_then(|convo| convo.to(world))
    else {
        return false;
    };
    let message = ChiefMessage::words(to, line.into());
    let request = message.request.clone();
    world.trigger(message);
    made_in(world, hub, Some(request))
}
