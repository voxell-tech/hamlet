//! The bridge from the client's entities to the world: every hamlet,
//! agent and road entity takes its place in the society the generator
//! draws.

mod crossing;
mod housing;
mod pointing;
mod roads;
mod state;
#[cfg(test)]
mod tests;
mod view;

use bevy::prelude::*;

use crate::chief::Settling;
use crate::generator::land::Land;
use crate::generator::society::{AgentId, Pair, Society, TownId};
use crate::harbor::Harbor;
use crate::sky::CameraSettings;
use crate::{RealmSystems, library};

/// Draws the client's hamlets, agents and roads in the generated
/// world.
pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(pointing::plugin)
        // The library takes its site after the chiefs. Both come
        // before any town is founded, as the Act step does before the
        // Bridge.
        .configure_sets(Update, (Settling, library::Raising).chain())
        .add_observer(housing::leave)
        .add_observer(housing::come_back)
        .add_observer(housing::move_out)
        .add_observer(housing::close)
        .add_observer(roads::unlay)
        .add_observer(roads::walk)
        .add_systems(
            Update,
            (
                housing::make_room.run_if(
                    any_with_component::<Unhoused>.and_then(
                        resource_changed::<Society>
                            .or_eager(resource_changed::<Land>),
                    ),
                ),
                housing::house,
                housing::seat,
                crossing::board.run_if(resource_exists::<Harbor>),
                (crossing::leave_home, crossing::cross)
                    .run_if(any_with_component::<crossing::Voyage>),
                housing::resite,
                roads::lay,
                state::show,
                view::tether.run_if(
                    resource_changed::<Society>
                        .or_eager(resource_changed::<Land>)
                        .or_eager(resource_changed::<CameraSettings>),
                ),
                view::aim,
            )
                .chain()
                .in_set(RealmSystems::Bridge),
        );
}

/// The hamlet whose town `town` is.
pub fn hamlet_of(world: &mut World, town: TownId) -> Option<Entity> {
    world
        .query::<(Entity, &Sited)>()
        .iter(world)
        .find(|(_, sited)| sited.0 == town)
        .map(|(hamlet, _)| hamlet)
}

/// The town of a hamlet, once the world has founded it.
pub fn town_of(world: &World, hamlet: Entity) -> Option<TownId> {
    world.get::<Sited>(hamlet).map(|sited| sited.0)
}

/// The town a hamlet is in the society.
#[derive(Component, Clone, Copy)]
pub struct Sited(pub TownId);

/// The agent of the society an agent entity is drawn as.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub struct Housed(pub AgentId);

/// An agent whose hamlet had no plot left for it.
#[derive(Component)]
struct Unhoused;

/// The house in the society that took this agent in as a wing.
#[derive(Component, Clone, Copy)]
struct Wing(AgentId);

/// A restored agent whose robot has yet to be seen coming back.
#[derive(Component)]
struct Returning;

/// The road of the society a road entity is drawn as.
#[derive(Component, Clone, Copy)]
struct Laid(Pair);
