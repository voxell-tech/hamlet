//! The agents the field names after `@`: every living agent but the
//! one the chat is with, those it has a channel to and the Hand
//! first, found by name or address and put in as their chips.

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::query::{With, Without};
use bevy::ecs::world::World;
use hamlet_client::realm::{
    self as agents, Identity, Left, LivesIn, channel_between,
};
use hamlet_protocol::signs::Sign;

use super::{Entry, Pick, Source};
use crate::convo::Convo;

pub(super) struct Mentions;

impl Source for Mentions {
    const SIGN: Sign = Sign::Mention;
    const LISTS: &str = "agents";

    fn entries(world: &World, rest: &str) -> Vec<Entry> {
        let Some(agent) =
            world.get_resource::<Convo>().map(|convo| convo.agent)
        else {
            return Vec::new();
        };
        let Some(mut living) = world.try_query_filtered::<
            (Entity, &Name, Option<&LivesIn>),
            (With<Identity>, Without<Left>),
        >() else {
            return Vec::new();
        };
        let rest = rest.to_lowercase();
        let hand = agents::hand(world);
        let mut found = living
            .iter(world)
            .filter(|(other, ..)| *other != agent)
            .filter_map(|(entity, address, home)| {
                let near = Some(entity) == hand
                    || channel_between(world, agent, entity)
                        .is_some();
                let shown = agents::shown_of(world, entity);
                let address = address.as_str().to_owned();
                let lower = shown.to_lowercase();
                let rank = if lower.starts_with(&rest) {
                    0
                } else if lower.contains(&rest) {
                    1
                } else if address.starts_with(&rest) {
                    2
                } else {
                    return None;
                };
                let hamlet =
                    home.map(|home| agents::shown_of(world, home.0));
                Some(((!near, rank), shown, address, hamlet))
            })
            .collect::<Vec<_>>();
        // Those it reaches first, then the closest matches.
        found.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        found
            .into_iter()
            .map(|(_, shown, address, hamlet)| Entry {
                title: format!("@{shown}"),
                detail: match hamlet {
                    Some(hamlet) => format!("{address} in {hamlet}"),
                    None => address.clone(),
                },
                pick: Pick::Reference {
                    words: match shown == address {
                        true => address.clone(),
                        false => format!("{address} ({shown})"),
                    },
                    address,
                },
            })
            .collect()
    }
}
