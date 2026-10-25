//! The roads between agents, and who walks them.

use bevy::prelude::*;
use hamlet_client::realm::{Carried, Road, RoadUse, Traffic};

use super::{Housed, Laid};
use crate::generator::Trips;
use crate::generator::society::{Pair, RoadLevel, Society};

fn level(level: RoadUse) -> RoadLevel {
    match level {
        RoadUse::Overgrown => RoadLevel::Overgrown,
        RoadUse::Dirt => RoadLevel::Dirt,
        RoadUse::Paved => RoadLevel::Paved,
        RoadUse::Rail => RoadLevel::Rail,
    }
}

/// Lays every road the world has not met, and follows the use of
/// those it has.
pub(super) fn lay(
    mut commands: Commands,
    mut society: ResMut<Society>,
    agents: Query<&Housed>,
    new: Query<(Entity, &Road, &Traffic), Without<Laid>>,
    used: Query<(&Laid, &Traffic), Changed<Traffic>>,
) {
    let set = |society: &mut ResMut<Society>,
               pair: Pair,
               level: RoadLevel| {
        if society.road(pair).is_some_and(|r| r.level != level) {
            society.set_level(pair, level);
        }
    };
    for (entity, road, traffic) in &new {
        let (Ok(a), Ok(b)) =
            (agents.get(road.ends.0), agents.get(road.ends.1))
        else {
            continue;
        };
        let pair = Pair::new(a.0, b.0);
        if society.road(pair).is_none() {
            society.add_road(a.0, b.0);
        }
        set(&mut society, pair, level(traffic.level));
        commands.entity(entity).try_insert(Laid(pair));
    }
    for (laid, traffic) in &used {
        set(&mut society, laid.0, level(traffic.level));
    }
}

/// A road went, with an agent or when a merge was undone.
pub(super) fn unlay(
    gone: On<Remove<Laid>>,
    roads: Query<&Laid>,
    mut society: ResMut<Society>,
) {
    let Ok(laid) = roads.get(gone.entity) else {
        return;
    };
    if society.road(laid.0).is_some() {
        society.roads.retain(|r| r.pair != laid.0);
    }
}

/// The sender of a message walks it over.
pub(super) fn walk(
    carried: On<Carried>,
    roads: Query<&Laid>,
    agents: Query<&Housed>,
    mut trips: ResMut<Trips>,
) {
    if let (Ok(laid), Ok(walker)) =
        (roads.get(carried.road), agents.get(carried.from))
    {
        trips.send(laid.0, walker.0);
    }
}
