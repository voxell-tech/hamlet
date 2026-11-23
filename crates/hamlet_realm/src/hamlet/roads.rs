//! The channels between agents, drawn as roads, and who walks them.

use bevy::prelude::*;
use hamlet_core::client::realm::{
    Carried, Channel, ChannelUse, Traffic,
};

use super::{Housed, Laid};
use crate::generator::Trips;
use crate::generator::society::{Pair, RoadLevel, Society};

fn level(level: ChannelUse) -> RoadLevel {
    match level {
        ChannelUse::Overgrown => RoadLevel::Overgrown,
        ChannelUse::Dirt => RoadLevel::Dirt,
        ChannelUse::Paved => RoadLevel::Paved,
        ChannelUse::Rail => RoadLevel::Rail,
    }
}

/// Lays a road for every channel the world has not met, and follows
/// the use of those it has.
pub(super) fn lay(
    mut commands: Commands,
    mut society: ResMut<Society>,
    agents: Query<&Housed>,
    new: Query<(Entity, &Channel, &Traffic), Without<Laid>>,
    used: Query<(&Laid, &Traffic), Changed<Traffic>>,
) {
    let set = |society: &mut ResMut<Society>,
               pair: Pair,
               level: RoadLevel| {
        if society.road(pair).is_some_and(|r| r.level != level) {
            society.set_level(pair, level);
        }
    };
    for (entity, channel, traffic) in &new {
        let (Ok(a), Ok(b)) =
            (agents.get(channel.ends.0), agents.get(channel.ends.1))
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

/// A channel went, with an agent or when a merge was undone, and
/// its road with it.
pub(super) fn unlay(
    gone: On<Remove<Laid>>,
    channels: Query<&Laid>,
    mut society: ResMut<Society>,
) {
    let Ok(laid) = channels.get(gone.entity) else {
        return;
    };
    if society.road(laid.0).is_some() {
        society.roads.retain(|r| r.pair != laid.0);
    }
}

/// The sender of a message walks it over.
pub(super) fn walk(
    carried: On<Carried>,
    channels: Query<&Laid>,
    agents: Query<&Housed>,
    mut trips: ResMut<Trips>,
) {
    if let (Ok(laid), Ok(walker)) =
        (channels.get(carried.channel), agents.get(carried.from))
    {
        trips.send(laid.0, walker.0);
    }
}
