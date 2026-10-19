//! The things of the world as the subjects commands are run on.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use hamlet_agents_api::agents;
use hamlet_realm_api::pick::Thing;
use hamlet_sdk::command::{Subject, kind};

/// The name a subject has when the world gives it none.
const MAYOR: &str = "The Mayor";

/// The subject `thing` is, with the entity it stands for as its id.
pub(crate) fn of(world: &mut World, thing: Thing) -> Subject {
    let named = |world: &World, what: &str, entity: Entity| {
        let name = agents::name_of(world, entity);
        Subject::new(what, &entity.to_bits().to_string(), &name)
    };
    match thing {
        Thing::Keeper | Thing::Cottage => {
            match agents::mayor(world) {
                Some(mayor) => named(world, kind::MAYOR, mayor),
                None => Subject::new(kind::MAYOR, "", MAYOR),
            }
        }
        Thing::Resident(agent) => named(world, kind::AGENT, agent),
        Thing::ResidentHouse(agent) => {
            named(world, kind::HOUSE, agent)
        }
        Thing::Ruin(agent) => named(world, kind::RUIN, agent),
        Thing::Common(hamlet)
        | Thing::Clockwork(hamlet)
        | Thing::Workbench(hamlet) => {
            named(world, kind::HAMLET, hamlet)
        }
        Thing::Chief(chief) => named(world, kind::CHIEF, chief),
        Thing::ChiefPlace(chief) => named(world, kind::HEARTH, chief),
        Thing::Library => {
            Subject::new(kind::LIBRARY, "", "The Library")
        }
    }
}

/// The subject of the bare ground.
pub(crate) fn ground() -> Subject {
    Subject::new(kind::GROUND, "", "")
}

/// The entity a subject stands for.
pub(crate) fn entity(subject: &Subject) -> Option<Entity> {
    Entity::try_from_bits(subject.id.parse().ok()?)
}

/// The thing a subject was made of, for a kind that has one.
pub(crate) fn thing(subject: &Subject) -> Option<Thing> {
    let entity = entity(subject);
    match (subject.kind.as_str(), entity) {
        (kind::AGENT, Some(agent)) => Some(Thing::Resident(agent)),
        (kind::HOUSE, Some(agent)) => {
            Some(Thing::ResidentHouse(agent))
        }
        (kind::RUIN, Some(agent)) => Some(Thing::Ruin(agent)),
        (kind::HAMLET, Some(hamlet)) => Some(Thing::Common(hamlet)),
        (kind::CHIEF, Some(chief)) => Some(Thing::Chief(chief)),
        (kind::HEARTH, Some(chief)) => Some(Thing::ChiefPlace(chief)),
        (kind::LIBRARY, _) => Some(Thing::Library),
        (kind::MAYOR, _) => Some(Thing::Keeper),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thing_is_a_subject_and_back() {
        let mut world = World::new();
        let agent =
            world.spawn(bevy::ecs::name::Name::new("Juniper")).id();
        for thing in [
            Thing::Resident(agent),
            Thing::ResidentHouse(agent),
            Thing::Ruin(agent),
            Thing::Common(agent),
            Thing::Chief(agent),
            Thing::ChiefPlace(agent),
            Thing::Library,
        ] {
            let subject = of(&mut world, thing);
            assert_eq!(super::thing(&subject), Some(thing));
        }
        let subject = of(&mut world, Thing::Resident(agent));
        assert_eq!(subject.name, "Juniper");
    }
}
