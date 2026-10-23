//! Robots out on the roads with messages, and the carts they ride
//! between towns.

use std::collections::HashMap;

use glam::{Quat, Vec3};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::Mesh;

use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::build::upload::{Draw, Unsent};
use crate::places::road::Drawn;
use crate::places::road::mesh::groove_lift;
use crate::realm::Ctx;
use crate::society::{AgentId, Pair};
use crate::world::palette::Palette;

/// A robot carrying a message along a road and coming back.
pub struct Trip {
    pub pair: Pair,
    pub walker: AgentId,
    /// Distance walked from the walker's own door.
    pub gone: f32,
    pub back: bool,
    /// True when it goes by cart: the road is one between towns, too
    /// long to walk.
    pub rides: bool,
}

impl Trip {
    /// The place of its robot on `road`, facing the way it goes.
    pub(super) fn stand(&self, road: &Drawn) -> Frame {
        let from_start = road.first == self.walker;
        let s = if from_start {
            self.gone
        } else {
            road.route.total() - self.gone
        };
        let (p, tan) = road.route.at(s);
        let tan = if from_start != self.back { tan } else { -tan };
        Frame::at(p + Vec3::Y * groove_lift(road.level, 0))
            .with_turn(Quat::from_rotation_y(tan.x.atan2(tan.z)))
    }
}

/// Messages on the roads right now, and the carts under those that
/// ride.
#[derive(Default)]
pub(super) struct Traffic {
    pub trips: Vec<Trip>,
    /// The cart's mesh, once it is handed over; none for one the
    /// engine refused.
    cart: Option<Option<Handle<Mesh>>>,
    carts: HashMap<AgentId, Entity>,
}

impl Traffic {
    /// Sends `walker` along its road to the other end and back,
    /// unless it is on the road already.
    pub fn send(&mut self, pair: Pair, walker: AgentId) {
        if pair.has(walker)
            && !self.trips.iter().any(|t| t.walker == walker)
        {
            self.trips.push(Trip {
                pair,
                walker,
                gone: 0.0,
                back: false,
                rides: false,
            });
        }
    }

    /// Walks and rides every trip `dt` seconds further along the
    /// roads drawn. `laying` says whether a road is still being
    /// built, for a trip to wait for.
    pub fn run(
        &mut self,
        ctx: &Ctx,
        drawn: &HashMap<Pair, Drawn>,
        laying: impl Fn(Pair) -> bool,
        dt: f32,
    ) {
        let society = ctx.society;
        let town = |agent| society.agent(agent).map(|a| a.town);
        let set = &ctx.land.set.road;
        self.trips.retain_mut(|trip| {
            trip.rides = town(trip.pair.0) != town(trip.pair.1);
            let Some(road) = drawn.get(&trip.pair) else {
                return laying(trip.pair);
            };
            let total = road.route.total();
            let speed = if trip.rides {
                // A cart gets going out of a town and slows into one.
                let near = trip.gone.min(total - trip.gone).max(0.0);
                let share = (near / set.ride_ramp).clamp(0.0, 1.0);
                // However far the towns lie apart, the ride is short.
                let ride = set.ride_speed.max(total / set.ride_time);
                set.walk_speed
                    + (ride - set.walk_speed) * share * (2.0 - share)
            } else {
                set.walk_speed
            };
            let step = speed * dt;
            if trip.back {
                trip.gone -= step;
                trip.gone > 0.0
            } else {
                trip.gone += step;
                trip.back = trip.gone >= total;
                true
            }
        });
    }

    /// Keeps a cart under every robot that is riding one.
    pub fn carts(
        &mut self,
        ctx: &Ctx,
        drawn: &HashMap<Pair, Drawn>,
        draw: &mut Draw,
    ) {
        let trips = &self.trips;
        self.carts.retain(|rider, cart| {
            let rides = trips
                .iter()
                .any(|trip| trip.walker == *rider && trip.rides);
            if !rides {
                draw.despawn(*cart);
            }
            rides
        });
        if self.trips.iter().all(|trip| !trip.rides) {
            return;
        }
        let Some(mesh) = self.cart_mesh(ctx.pal, draw) else {
            return;
        };
        for trip in self.trips.iter().filter(|trip| trip.rides) {
            let Some(road) = drawn.get(&trip.pair) else {
                continue;
            };
            let cart =
                *self.carts.entry(trip.walker).or_insert_with(|| {
                    let cart = draw.spawn(None, 0);
                    draw.draw(cart, &mesh, &ctx.stock.matte);
                    cart
                });
            draw.place(cart, trip.stand(road));
        }
    }

    /// The cart's mesh, handed over the first time it is wanted.
    fn cart_mesh(
        &mut self,
        pal: &Palette,
        draw: &mut Draw,
    ) -> Option<Handle<Mesh>> {
        if self.cart.is_none() {
            match cart(pal).upload(draw) {
                Ok(mesh) => self.cart = Some(Some(mesh)),
                Err(Unsent::Full(_)) => return None,
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_bevy::warn(&format!(
                        "a cart: {why}"
                    ));
                    self.cart = Some(None);
                }
            }
        }
        self.cart.clone().flatten()
    }
}

/// A cart for the roads between towns: a plank box on four wheels
/// that a robot stands in.
fn cart(pal: &Palette) -> Mb {
    let mut mb = Mb::new();
    let (wide, long, deep) = (0.42, 0.56, 0.2);
    let floor = 0.14;
    let plank = 0.035;
    let turn = Quat::IDENTITY;
    // The bed, and a wall on each side of it.
    mb.rbox(
        Vec3::new(0.0, floor, 0.0),
        Vec3::new(wide, plank, long),
        0.012,
        turn,
        pal.timber_dark,
        0.3,
    );
    for side in [-1.0, 1.0] {
        mb.rbox(
            Vec3::new(side * wide, floor + deep, 0.0),
            Vec3::new(plank, deep, long),
            0.012,
            turn,
            pal.timber,
            0.2,
        );
        mb.rbox(
            Vec3::new(0.0, floor + deep, side * long),
            Vec3::new(wide, deep, plank),
            0.012,
            turn,
            pal.timber,
            0.2,
        );
        // A brass band along the top of each long side.
        mb.rbox(
            Vec3::new(side * wide, floor + 2.0 * deep, 0.0),
            Vec3::new(plank * 1.3, 0.018, long * 1.02),
            0.006,
            turn,
            pal.brass,
            0.0,
        );
        for end in [-1.0, 1.0] {
            let hub = Vec3::new(side * wide, 0.12, end * long * 0.62);
            mb.tube(
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
    mb
}
