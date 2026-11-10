//! A hire that gets a house after the world was first seen crosses
//! the sea to it: it walks from its scholar's house in the city to
//! the pier, sails to the hamlet's pier and walks to its porch, while
//! its robot is away.

use bevy::prelude::*;
use hamlet_client::realm::{AgentNumber, OfScholar, Scholar, Seen};
use hamlet_protocol::client::Arrival;

use super::Housed;
use crate::Clock;
use crate::city::houses;
use crate::generator::land::{IslandId, Land};
use crate::generator::society::AgentId;
use crate::generator::{Away, Robot};
use crate::harbor::ferry::{self, Ferry};
use crate::harbor::{Boat, Harbor};
use crate::keeper::{self, Kit, Motion, Trip, Who};

/// Where a traveler sits in a boat, in the boat's frame.
const SEAT: Vec3 = Vec3::new(0.0, -0.1, -0.3);

/// Where a traveler has got to.
#[derive(Clone, Copy, Debug)]
enum Stage {
    /// Walking to the city's pier.
    Pier,
    /// Sailing in the ferry.
    Aboard(Entity),
    /// Walking from the hamlet's pier to its porch.
    Ashore,
}

/// The figure of a hire on its way to its house.
#[derive(Component, Debug)]
pub(super) struct Voyage {
    /// The hire's entity.
    hire: Entity,
    /// The agent of the society whose robot is away.
    home: AgentId,
    stage: Stage,
}

/// Sends each hire housed after the first state to its house by
/// way of the sea, starting from its scholar's door or, when the
/// scholar has no house, from the pier.
pub(super) fn board(
    mut commands: Commands,
    clock: Res<Clock>,
    motion: Res<Motion>,
    land: Res<Land>,
    harbor: Option<Res<Harbor>>,
    kit: Res<Kit>,
    seen: Option<Res<Seen>>,
    mut primed: Local<bool>,
    hires: Query<
        (Entity, &Housed, &AgentNumber, Option<&OfScholar>),
        Added<Housed>,
    >,
    scholars: Query<&Scholar>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if seen.is_none() {
        return;
    }
    // What was housed in the first state did not just arrive.
    if !*primed {
        *primed = true;
        return;
    }
    let Some(harbor) = harbor else {
        return;
    };
    for (hire, housed, number, of) in &hires {
        // The mayor's hall is founded with its hamlet.
        if number.0 == 0 {
            continue;
        }
        let scholar = of.and_then(|of| {
            scholars.iter().find(|scholar| scholar.0.id == of.0)
        });
        // A blank hire is of no house in the city: it is at its own
        // at once.
        if scholar.is_some_and(|s| s.0.arrival == Arrival::Apart) {
            continue;
        }
        let door = scholar
            .filter(|scholar| scholar.0.left.is_none())
            .and_then(|scholar| scholar.0.house.as_ref())
            .map(|house| houses::door(&land, house));
        let way = match door {
            Some(door) => {
                harbor.to_gangway(&land, IslandId::City, door)
            }
            None => {
                let deck = land.set.land.water_y + Harbor::DECK;
                [
                    harbor.queue(IslandId::City, 0),
                    harbor.gangway(IslandId::City),
                ]
                .map(|p| Vec3::new(p.x, deck, p.y))
                .to_vec()
            }
        };
        let traveler = keeper::spawn_agent(
            &mut commands,
            &kit,
            &mut materials,
            Who::agent(housed.0.0 * 7 + 3),
            Transform::from_translation(way[0]),
            None,
        );
        commands.entity(traveler).insert((
            Trip::journey(way, clock.t, &motion),
            Voyage {
                hire,
                home: housed.0,
                stage: Stage::Pier,
            },
        ));
    }
}

/// Keeps the robot of each traveler out of sight.
pub(super) fn leave_home(
    mut commands: Commands,
    voyages: Query<&Voyage>,
    robots: Query<(Entity, &Robot), Without<Away>>,
) {
    for voyage in &voyages {
        if let Some((robot, _)) = robots
            .iter()
            .find(|(_, robot)| robot.agent == voyage.home)
        {
            commands.entity(robot).try_insert(Away);
        }
    }
}

/// Carries each traveler from one stage to the next, and shows its
/// robot again once it has come home.
pub(super) fn cross(
    mut commands: Commands,
    clock: Res<Clock>,
    motion: Res<Motion>,
    land: Res<Land>,
    harbor: Option<Res<Harbor>>,
    mut voyages: Query<(Entity, &mut Voyage, &Trip)>,
    boats: Query<(&Boat, &Mesh3d, &MeshMaterial3d<StandardMaterial>)>,
    ferries: Query<&Trip, With<Ferry>>,
    robots: Query<(Entity, &Robot)>,
    hires: Query<(), With<Housed>>,
) {
    let now = clock.t;
    let water = land.set.land.water_y;
    for (entity, mut voyage, trip) in &mut voyages {
        let robot = robots
            .iter()
            .find(|(_, robot)| robot.agent == voyage.home)
            .map(|(robot, _)| robot);
        let finish = |commands: &mut Commands| {
            commands.entity(entity).despawn();
            if let Some(robot) = robot {
                commands.entity(robot).try_remove::<Away>();
            }
        };
        let ferry = match voyage.stage {
            Stage::Aboard(ferry) => Some(ferry),
            _ => None,
        };
        let lost = harbor.is_none()
            || !hires.contains(voyage.hire)
            || ferry.is_some_and(|ferry| ferries.get(ferry).is_err());
        if lost {
            finish(&mut commands);
            if let Some(ferry) = ferry {
                commands.entity(ferry).try_despawn();
            }
            continue;
        }
        let Some(harbor) = harbor.as_deref() else {
            continue;
        };
        match voyage.stage {
            Stage::Pier => {
                if !trip.has_arrived(now) {
                    continue;
                }
                let Some((_, mesh, material)) = boats
                    .iter()
                    .find(|(boat, ..)| boat.0 == IslandId::City)
                else {
                    finish(&mut commands);
                    continue;
                };
                let sail = ferry::course(
                    harbor,
                    water,
                    IslandId::City,
                    now,
                    &motion,
                );
                let ferry = ferry::launch(
                    &mut commands,
                    (mesh, material),
                    IslandId::City,
                    sail,
                );
                commands.entity(entity).insert((
                    ChildOf(ferry),
                    Transform::from_translation(SEAT),
                    // Sitting still, as a trip of no length is.
                    Trip::new(vec![SEAT, SEAT], now, &motion),
                ));
                voyage.stage = Stage::Aboard(ferry);
            }
            Stage::Aboard(ferry) => {
                let Ok(sail) = ferries.get(ferry) else {
                    continue;
                };
                let Some(porch) = robots
                    .iter()
                    .find(|(_, robot)| robot.agent == voyage.home)
                    .map(|(_, robot)| robot.porch())
                else {
                    continue;
                };
                if !sail.has_arrived(now) {
                    continue;
                }
                // The hamlet's ground has no way between its houses
                // for a walker not at one, so it walks straight.
                let way = harbor.walk_ashore(
                    &land,
                    IslandId::Hamlet,
                    porch.xz(),
                );
                commands.entity(entity).remove::<ChildOf>().insert((
                    Transform::from_translation(way[0]),
                    Trip::journey(way, now, &motion),
                ));
                ferry::send_home(
                    &mut commands,
                    ferry,
                    ferry::course(
                        harbor,
                        water,
                        IslandId::Hamlet,
                        now,
                        &motion,
                    ),
                );
                voyage.stage = Stage::Ashore;
            }
            Stage::Ashore => {
                if trip.has_arrived(now) {
                    finish(&mut commands);
                }
            }
        }
    }
}
