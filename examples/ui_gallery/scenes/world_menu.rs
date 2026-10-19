//! The menu of a thing in the world, open over the panels.

use std::path::PathBuf;

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use hamlet_core::agents::{Busy, Citizen, Left, Settlement};
use hamlet_realm::pick::{Also, Thing};
use plugin_menus::world_menu;

/// What the menu is of.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// An agent at work, with another agent chosen beside it.
    Agent,
    Mayor,
    Hamlet,
    /// The remains of the house of an agent that left.
    Ruin,
    Chief,
    Library,
    /// The bare ground.
    Ground,
}

/// Opens the menu of `place`, where `agent` is the agent of the
/// scene.
pub(super) fn put(world: &mut World, place: Place, agent: Entity) {
    let at = Vec2::new(760.0, 180.0);
    let thing = match place {
        Place::Agent => {
            world.entity_mut(agent).insert(Busy);
            let other =
                world.spawn((Name::new("Hazel"), Citizen(6))).id();
            world.insert_resource(Also(Some(Thing::Resident(other))));
            Thing::Resident(agent)
        }
        Place::Mayor => Thing::Keeper,
        Place::Hamlet => {
            let hamlet = world
                .spawn((
                    Name::new("Pear Tree Hollow"),
                    Settlement {
                        home: PathBuf::new(),
                        name: "orchard".into(),
                    },
                ))
                .id();
            Thing::Common(hamlet)
        }
        Place::Ruin => {
            let gone = world
                .spawn((Name::new("Rowan"), Citizen(7), Left))
                .id();
            Thing::Ruin(gone)
        }
        Place::Chief => {
            Thing::Chief(world.spawn(Name::new("Wren")).id())
        }
        Place::Library => Thing::Library,
        Place::Ground => {
            return world_menu::show_ground(world, at);
        }
    };
    world_menu::show(world, thing, at);
}
