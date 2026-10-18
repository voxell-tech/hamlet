//! Towns founded and agents housed: who lives on which plot, who left
//! and who came back.

use std::collections::HashMap;

use bevy::prelude::*;
use hamlet_core::agents::{
    Citizen, Left, Lived, LivesIn, MergedInto, Settlement, Spot,
};

use super::{Housed, Returning, Sited, Unhoused, Wing};
use crate::generator::land::Land;
use crate::generator::society::{AgentId, Society};
use crate::generator::{Passage, Robot};
use crate::keeper::Act;

/// Archives `gone` in the society: a wing on the house that took it
/// in, or a ruin. Returns the house, when there is a wing.
fn archive(
    society: &mut Society,
    gone: AgentId,
    host: Option<AgentId>,
) -> Option<AgentId> {
    let host = host.filter(|host| society.merge(*host, gone));
    if host.is_none() {
        society.remove_agent(gone);
    }
    host
}

/// Lets an agent that found no plot try again once its hamlet has
/// one.
pub(super) fn make_room(
    mut commands: Commands,
    land: Res<Land>,
    society: Res<Society>,
    hamlets: Query<&Sited>,
    unhoused: Query<(Entity, &LivesIn), With<Unhoused>>,
) {
    for (agent, lives) in &unhoused {
        let room = hamlets
            .get(lives.0)
            .is_ok_and(|sited| society.has_room(&land, sited.0));
        if room {
            commands.entity(agent).try_remove::<Unhoused>();
        }
    }
}

/// Founds each hamlet's town and gives every agent the world has
/// not met a plot.
pub(super) fn house(
    mut commands: Commands,
    mut land: ResMut<Land>,
    mut society: ResMut<Society>,
    hamlets: Query<
        (Entity, Option<&Sited>, Option<&Spot>),
        With<Settlement>,
    >,
    housed: Query<(Entity, &Housed)>,
    homeless: Query<
        (
            Entity,
            &Citizen,
            &LivesIn,
            Has<Left>,
            Option<&MergedInto>,
            &Lived,
        ),
        (Without<Housed>, Without<Unhoused>),
    >,
    names: Query<&Name>,
) {
    if homeless.is_empty() {
        return;
    }
    let mut homes = housed
        .iter()
        .map(|(entity, housed)| (entity, housed.0))
        .collect::<HashMap<_, _>>();
    for (hamlet, sited, spot) in &hamlets {
        let town = match sited {
            Some(sited) => sited.0,
            None => {
                // At the spot its chief set it down, or on the first
                // free site.
                let town = match spot.and_then(|spot| spot.0) {
                    Some((x, z)) => society
                        .found_town_at(&mut land, Vec2::new(x, z)),
                    None => society.found_town(&mut land),
                };
                commands.entity(hamlet).try_insert(Sited(town));
                town
            }
        };
        let Some(leader) = society.town(town).map(|t| t.leader)
        else {
            continue;
        };
        // Agents are housed, and those who left are seen out, in the
        // order it happened, so that a newcomer takes the lowest plot
        // free at the time, as it did when the world last saw it.
        let new = homeless
            .iter()
            .filter(|(_, _, lives, ..)| lives.0 == hamlet)
            .collect::<Vec<_>>();
        let mut story = new
            .iter()
            .flat_map(|(entity, citizen, _, left, into, lived)| {
                let came = (lived.since, citizen.0, false);
                let went = left.then(|| {
                    let at = lived.until.unwrap_or(u64::MAX);
                    (at, citizen.0, true)
                });
                let host = into.map(|m| m.0);
                core::iter::once(came)
                    .chain(went)
                    .map(move |when| (when, *entity, host))
            })
            .collect::<Vec<_>>();
        story.sort_by_key(|(when, ..)| *when);
        for ((_, number, goes), entity, host) in story {
            if !goes {
                // The hall is the first mayor's house, whoever
                // holds the office.
                let agent = if number == 0 {
                    Some(leader)
                } else if society.has_room(&land, town) {
                    society.add_agent(&land, town)
                } else {
                    None
                };
                match agent {
                    Some(agent) => {
                        homes.insert(entity, agent);
                        commands
                            .entity(entity)
                            .try_insert(Housed(agent));
                    }
                    None => {
                        let name = names
                            .get(entity)
                            .map_or("an agent", Name::as_str);
                        warn!(
                            "hamlet: no plot is left for {name}, \
                             which has no house until one is free"
                        );
                        commands.entity(entity).try_insert(Unhoused);
                    }
                }
                continue;
            }
            // Those who left before the world met them are not
            // seen going.
            let Some(gone) = homes.get(&entity).copied() else {
                continue;
            };
            let host =
                host.and_then(|host| homes.get(&host)).copied();
            if let Some(host) = archive(&mut society, gone, host) {
                commands.entity(entity).try_insert(Wing(host));
            }
        }
    }
}

/// Sets each town down where the record says its chief put it. A
/// place the ground cannot take is left out, and the town stays.
pub(super) fn resite(
    mut land: ResMut<Land>,
    mut society: ResMut<Society>,
    hamlets: Query<(&Sited, &Spot, &Settlement), Changed<Spot>>,
) {
    for (sited, spot, settlement) in &hamlets {
        let Some((x, z)) = spot.0 else {
            continue;
        };
        let to = Vec2::new(x, z);
        let stands = society
            .town(sited.0)
            .map(|town| land.sites[town.site].at);
        if stands.is_none_or(|at| at == to) {
            continue;
        }
        if let Err(why) = society.move_town(&mut land, sited.0, to) {
            eprintln!(
                "hamlet: the town of {} stays where it is: {to} is \
                 {why:?}",
                settlement.name
            );
        }
    }
}

/// An agent left: a ruin, or a wing on the house that took it in,
/// and its robot is seen going.
pub(super) fn leave(
    left: On<Add, Left>,
    mut commands: Commands,
    mut society: ResMut<Society>,
    agents: Query<(&Housed, Option<&MergedInto>)>,
    robots: Query<(Entity, &Robot), Without<Passage>>,
) {
    let Ok((gone, into)) = agents.get(left.entity) else {
        return;
    };
    let host = into
        .and_then(|m| agents.get(m.0).ok())
        .map(|(host, _)| host.0);
    let mut motions = vec![];
    match archive(&mut society, gone.0, host) {
        Some(host) => {
            commands.entity(left.entity).try_insert(Wing(host));
            motions.push((gone.0, Act::MergeInto));
            motions.push((host, Act::MergeHost));
        }
        None => motions.push((gone.0, Act::Leave)),
    }
    for (entity, robot) in &robots {
        if let Some((_, act)) =
            motions.iter().find(|(agent, _)| *agent == robot.agent)
        {
            commands.entity(entity).try_insert(Passage::new(*act));
        }
    }
}

/// An agent came back: its house stands again, and the house a
/// merged one was split from loses the wing.
pub(super) fn come_back(
    back: On<Remove, Left>,
    mut commands: Commands,
    land: Res<Land>,
    mut society: ResMut<Society>,
    agents: Query<(&Housed, Option<&Wing>)>,
) {
    let Ok((agent, wing)) = agents.get(back.entity) else {
        return;
    };
    match wing {
        Some(wing) => {
            society.split(&land, wing.0, agent.0);
        }
        None => {
            society.restore(&land, agent.0);
        }
    }
    // The agent itself may be going with its hamlet.
    if let Ok(mut entity) = commands.get_entity(back.entity) {
        entity.try_remove::<Wing>().try_insert(Returning);
    }
}

/// An agent is gone from its hamlet for good: it moved to another,
/// or the hamlet was removed. Its house there is a ruin.
pub(super) fn move_out(
    gone: On<Remove, Housed>,
    agents: Query<&Housed>,
    mut society: ResMut<Society>,
) {
    if let Ok(agent) = agents.get(gone.entity)
        && society.agent(agent.0).is_some_and(|a| !a.archived)
    {
        society.remove_agent(agent.0);
    }
}

/// A hamlet is gone: its town goes with it, hall, common and all.
pub(super) fn close(
    gone: On<Remove, Sited>,
    hamlets: Query<&Sited>,
    mut society: ResMut<Society>,
) {
    if let Ok(sited) = hamlets.get(gone.entity) {
        society.remove_town(sited.0);
    }
}
