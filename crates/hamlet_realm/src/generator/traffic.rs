//! Robots out on the roads with messages, and where each robot
//! stands.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::land::Land;
use super::loading::Loading;
use super::society::{AgentId, Pair, Society};
use super::sync::{Drawn, RoadDrawn};
use super::{Kit, road};
use crate::Clock;
use crate::meshes::Meshing;
use crate::palette::Palette;

/// A robot carrying a message along a road and coming back.
pub struct Trip {
    pub pair: Pair,
    pub walker: AgentId,
    /// Distance walked from the walker's own door.
    pub(super) gone: f32,
    pub(super) back: bool,
    /// A share of the road to start at, until the road's length is
    /// known.
    part: Option<f32>,
    /// True when it goes by cart: the road is one between towns, too
    /// long to walk.
    pub(super) rides: bool,
}

impl Trip {
    /// The place of its robot on `road`, and the way it faces.
    pub(super) fn stand(&self, road: &RoadDrawn) -> (Vec3, Quat) {
        let from_start = road.first == self.walker;
        let s = if from_start {
            self.gone
        } else {
            road.route.total() - self.gone
        };
        let (p, tan) = road.route.at(s);
        let tan = if from_start != self.back { tan } else { -tan };
        (
            p + Vec3::Y * road::groove_lift(road.level, 0),
            Quat::from_rotation_y(tan.x.atan2(tan.z)),
        )
    }
}

/// Messages on the road right now.
#[derive(Resource, Default)]
pub struct Trips {
    pub trips: Vec<Trip>,
}

impl Trips {
    /// Sends `walker` along its road to the other end and back.
    pub fn send(&mut self, pair: Pair, walker: AgentId) {
        self.send_part(pair, walker, 0.0);
    }

    /// As [`Trips::send`], with the walker already `part` of the
    /// way along.
    pub fn send_part(
        &mut self,
        pair: Pair,
        walker: AgentId,
        part: f32,
    ) {
        if pair.has_end(walker)
            && !self.trips.iter().any(|t| t.walker == walker)
        {
            self.trips.push(Trip {
                pair,
                walker,
                gone: 0.0,
                back: false,
                part: Some(part),
                rides: false,
            });
        }
    }
}

pub(super) fn run_traffic(
    time: Res<Time>,
    clock: Res<Clock>,
    land: Res<Land>,
    drawn: Res<Drawn>,
    loading: Res<Loading>,
    society: Res<Society>,
    mut traffic: ResMut<Trips>,
) {
    let town = |agent| society.agent(agent).map(|a| a.town);
    for trip in &mut traffic.trips {
        if let Some(road) = drawn.roads.get(&trip.pair)
            && let Some(part) = trip.part.take()
        {
            trip.gone = part * road.route.total();
        }
        trip.rides = town(trip.pair.0) != town(trip.pair.1);
    }
    if clock.frozen || traffic.trips.is_empty() {
        return;
    }
    let set = &land.set.road;
    traffic.trips.retain_mut(|trip| {
        let Some(road) = drawn.roads.get(&trip.pair) else {
            // It waits for a road that is still being built.
            return loading.builds.road(trip.pair).is_some();
        };
        let speed = if trip.rides {
            // A cart gets going out of a town and slows into one.
            let total = road.route.total();
            let near = trip.gone.min(total - trip.gone).max(0.0);
            let share = (near / set.ride_ramp).clamp(0.0, 1.0);
            // However far the towns lie apart, the ride is short.
            let ride = set.ride_speed.max(total / set.ride_time);
            set.walk_speed
                + (ride - set.walk_speed) * share * (2.0 - share)
        } else {
            set.walk_speed
        };
        let step = speed * time.delta_secs();
        if trip.back {
            trip.gone -= step;
            trip.gone > 0.0
        } else {
            trip.gone += step;
            trip.back = trip.gone >= road.route.total();
            true
        }
    });
}

/// A cart for the roads between towns: a plank box on four wheels
/// that a robot stands in.
pub(super) fn cart(pal: &Palette) -> Meshing {
    let mut meshing = Meshing::new();
    let (wide, long, deep) = (0.42, 0.56, 0.2);
    let floor = 0.14;
    let plank = 0.035;
    let turn = Quat::IDENTITY;
    // The bed, and a wall on each side of it.
    meshing.rbox(
        Vec3::new(0.0, floor, 0.0),
        Vec3::new(wide, plank, long),
        0.012,
        turn,
        pal.timber_dark,
        0.3,
    );
    for side in [-1.0, 1.0] {
        meshing.rbox(
            Vec3::new(side * wide, floor + deep, 0.0),
            Vec3::new(plank, deep, long),
            0.012,
            turn,
            pal.timber,
            0.2,
        );
        meshing.rbox(
            Vec3::new(0.0, floor + deep, side * long),
            Vec3::new(wide, deep, plank),
            0.012,
            turn,
            pal.timber,
            0.2,
        );
        // A brass band along the top of each long side.
        meshing.rbox(
            Vec3::new(side * wide, floor + 2.0 * deep, 0.0),
            Vec3::new(plank * 1.3, 0.018, long * 1.02),
            0.006,
            turn,
            pal.brass,
            0.0,
        );
        for end in [-1.0, 1.0] {
            let hub = Vec3::new(side * wide, 0.12, end * long * 0.62);
            meshing.tube(
                hub,
                hub + Vec3::X * side * 0.07,
                0.12,
                0.12,
                10,
                pal.brass,
                true,
            );
        }
    }
    meshing
}

/// Keeps a cart under every robot that is riding one.
pub(super) fn carts(
    mut commands: Commands,
    kit: Res<Kit>,
    drawn: Res<Drawn>,
    traffic: Res<Trips>,
    mut carts: Local<HashMap<AgentId, Entity>>,
    mut places: Query<&mut Transform>,
) {
    carts.retain(|rider, cart| {
        let rides = traffic
            .trips
            .iter()
            .any(|trip| trip.walker == *rider && trip.rides);
        if !rides {
            commands.entity(*cart).try_despawn();
        }
        rides
    });
    for trip in traffic.trips.iter().filter(|trip| trip.rides) {
        let Some(road) = drawn.roads.get(&trip.pair) else {
            continue;
        };
        let (at, turned) = trip.stand(road);
        let place =
            Transform::from_translation(at).with_rotation(turned);
        match carts.get(&trip.walker) {
            Some(cart) => {
                if let Ok(mut was) = places.get_mut(*cart)
                    && *was != place
                {
                    *was = place;
                }
            }
            None => {
                let cart = commands
                    .spawn((
                        Mesh3d(kit.cart.clone()),
                        MeshMaterial3d(kit.matte.clone()),
                        place,
                    ))
                    .id();
                carts.insert(trip.walker, cart);
            }
        }
    }
}
