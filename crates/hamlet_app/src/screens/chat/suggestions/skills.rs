//! The skills the field names after `$`: the library's and those in
//! the house of the agent the chat is with.

use bevy::ecs::world::World;
use hamlet_client::realm::{HouseSkills, Skills as Library};
use hamlet_protocol::signs::Sign;

use super::{Entry, Pick, Source};
use crate::convo::Convo;

pub(super) struct Skills;

impl Source for Skills {
    const SIGN: Sign = Sign::Skill;
    const LISTS: &str = "skills";

    fn entries(world: &World, rest: &str) -> Vec<Entry> {
        let rest = rest.to_lowercase();
        let mut found = known(world)
            .into_iter()
            .filter(|(name, _)| name.to_lowercase().contains(&rest))
            .collect::<Vec<_>>();
        found.sort_by_key(|(name, _)| {
            (!name.to_lowercase().starts_with(&rest), name.clone())
        });
        found
            .into_iter()
            .map(|(name, description)| Entry {
                title: format!("${name}"),
                detail: description,
                pick: Pick::Chip {
                    shown: format!("${name}"),
                    sent: format!("${name}"),
                },
            })
            .collect()
    }
}

/// Each skill the agent the chat is with can use, with what it does.
fn known(world: &World) -> Vec<(String, String)> {
    let mut known = world
        .get_resource::<Library>()
        .map(|library| {
            library
                .0
                .iter()
                .map(|skill| {
                    (skill.name.clone(), skill.description.clone())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let house = world
        .get_resource::<Convo>()
        .and_then(|convo| world.get::<HouseSkills>(convo.agent));
    for name in house.iter().flat_map(|house| house.0.iter()) {
        if !known.iter().any(|(known, _)| known == name) {
            known.push((name.clone(), String::new()));
        }
    }
    known
}
