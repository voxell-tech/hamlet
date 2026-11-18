//! The commands the field lists after `/`, each run as its control
//! does when the line is sent. Only those about the agent the chat is
//! with are here: the rest have a menu of their own.

use bevy::ecs::world::World;
use hamlet_client::messages::ChiefMessage;
use hamlet_client::realm::{ActionUsage, Actions};
use hamlet_protocol::signs::Sign;

use super::super::replied;
use super::{Entry, Pick, Source};
use crate::convo::Convo;
use crate::screens::hub::thread::compact_now;
use crate::world_menu;

/// Each command listed, with what runs it.
const LISTED: [(&str, fn(&mut World)); 5] = [
    ("compact", |world| {
        let agent = world.resource::<Convo>().agent;
        compact_now(world, agent);
    }),
    ("retry", |world| say(world, "/retry")),
    ("stop", |world| {
        let agent = world.resource::<Convo>().agent;
        let request = world_menu::stop(world, agent);
        replied(world, request);
    }),
    ("reflect", |world| say(world, "/reflect")),
    ("model", |world| {
        let agent = world.resource::<Convo>().agent;
        world_menu::model::open(world, agent);
    }),
];

pub(super) struct Commands;

impl Source for Commands {
    const SIGN: Sign = Sign::Command;
    const LISTS: &str = "commands";

    fn entries(world: &World, rest: &str) -> Vec<Entry> {
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

/// Runs the command `line` is, when it is one listed and allowed,
/// and says whether it ran.
pub(in crate::screens::chat) fn run(
    world: &mut World,
    line: &str,
) -> bool {
    let Some(name) = line.strip_prefix(Sign::Command.char()) else {
        return false;
    };
    let found = LISTED.into_iter().find(|(listed, _)| {
        *listed == name && Actions::is_allowed(world, listed)
    });
    if let Some((_, run)) = found {
        run(world);
    }
    found.is_some()
}

/// Sends the command `line` to the agent the chat is with.
fn say(world: &mut World, line: &str) {
    if let Some(to) = world.resource::<Convo>().to(world) {
        world.trigger(ChiefMessage::words(to, line.into()));
    }
}
