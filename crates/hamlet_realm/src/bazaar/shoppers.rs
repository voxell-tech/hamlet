//! The shoppers of the bazaar, who go from stall to stall, look at
//! what is for sale and linger over tea.

use std::f32::consts::PI;

use bevy::prelude::*;

use super::build::{FLOOR, TEA, at_stall, placing};
use super::{BazaarSettings, Stall};
use crate::Clock;
use crate::chief::mascot::{self, Doing, Pose, Stance};
use crate::chief::{ChiefSettings, Cloak, Hat, Holds, Light, Look};

/// How far before a stall's middle a shopper stands at it.
const BEFORE: f32 = 2.0;

/// How far from the tea table a shopper stands at it.
const SIPS: f32 = 1.0;

/// How far out the promenade's gaps between stalls are passed by,
/// and the way in at the gate.
const OUT: f32 = 10.0;
const GATE: f32 = 9.7;

/// The seconds each shopper's round is ahead of the first's, so that
/// they are seldom all still at once.
const AHEAD: [f32; 8] = [0.0, 7.5, 3.2, 11.0, 5.5, 2.1, 9.3, 14.0];

/// A shopper: no chief and no agent, only one who goes its round of
/// the bazaar.
#[derive(Component, Clone, Copy, Debug)]
pub struct Shopper {
    /// Its round, by its place in [`ROUNDS`].
    pub round: usize,
}

/// Where a shopper stops.
#[derive(Clone, Copy, Debug)]
enum Spot {
    /// Before a stall's counter, this far along it from its middle,
    /// facing it.
    Stall(Stall, f32),
    /// At the tea table, facing it.
    Tea,
    /// On the plaza at this many degrees round from the gate and this
    /// far out, facing the way it came.
    Ring(f32, f32),
    /// At this spot, facing this way.
    At(f32, f32, f32),
}

/// A stop on a shopper's round: where it stands, the seconds it stays
/// and what it does meanwhile.
#[derive(Clone, Copy, Debug)]
struct Stop {
    spot: Spot,
    stays: f32,
    doing: Doing,
}

const fn stop(spot: Spot, stays: f32, doing: Doing) -> Stop {
    Stop { spot, stays, doing }
}

/// A stop it only passes, to keep clear of what stands between.
const fn by(degrees: f32, out: f32) -> Stop {
    stop(Spot::Ring(degrees, out), 0.0, Doing::Walk)
}

impl Stop {
    /// Where it is, from the bazaar's middle, and the way it faces
    /// there, if it is a way of its own.
    fn place(&self) -> (Vec2, Option<f32>) {
        let facing = |way: Vec3| Some(way.x.atan2(way.z));
        match self.spot {
            Spot::Stall(stall, along) => {
                let (_, turn) = placing(stall);
                (
                    at_stall(stall, Vec3::new(along, 0.0, BEFORE))
                        .xz(),
                    facing(turn * Vec3::NEG_Z),
                )
            }
            Spot::Tea => {
                let (at, turn) = TEA;
                let turn = Quat::from_rotation_y(turn);
                (
                    (at + turn * Vec3::NEG_Z * SIPS).xz(),
                    facing(turn * Vec3::Z),
                )
            }
            Spot::Ring(degrees, out) => {
                let a = degrees.to_radians();
                (Vec2::new(a.sin(), a.cos()) * out, None)
            }
            Spot::At(x, z, facing) => (Vec2::new(x, z), Some(facing)),
        }
    }

    fn at(&self) -> Vec2 {
        self.place().0
    }
}

/// Each shopper's dress.
pub(super) const LOOKS: [Look; 8] = [
    Look {
        cloak: Cloak::Rose,
        light: Light::ALL[2],
        hat: Hat::Hood,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Moss,
        light: Light::ALL[3],
        hat: Hat::Wreath,
        holds: Holds::Book,
    },
    Look {
        cloak: Cloak::River,
        light: Light::ALL[4],
        hat: Hat::Pointed,
        holds: Holds::Teacup,
    },
    Look {
        cloak: Cloak::Cream,
        light: Light::ALL[0],
        hat: Hat::Crown,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Slate,
        light: Light::ALL[1],
        hat: Hat::Hood,
        holds: Holds::Book,
    },
    Look {
        cloak: Cloak::Ember,
        light: Light::ALL[3],
        hat: Hat::Wreath,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Plum,
        light: Light::ALL[2],
        hat: Hat::Pointed,
        holds: Holds::Quill,
    },
    Look {
        cloak: Cloak::Mustard,
        light: Light::ALL[4],
        hat: Hat::Bare,
        holds: Holds::Teacup,
    },
];

/// Each shopper's round. Between stalls each goes out round the
/// promenade, by the gaps between them and the gate, so that none
/// goes through a stall, its keeper, the pavilion or the tea table.
const ROUNDS: [&[Stop]; 8] = [
    &[
        stop(Spot::Stall(Stall::Hamlets, -0.3), 6.0, Doing::Idle),
        by(300.0, OUT),
        by(330.0, OUT - 1.0),
        by(0.0, GATE),
        stop(Spot::Stall(Stall::Models, -0.4), 7.0, Doing::Gaze),
        by(0.0, GATE),
        stop(Spot::At(0.6, 5.6, PI), 5.0, Doing::Idle),
        by(0.0, GATE),
        by(330.0, OUT - 1.0),
        by(300.0, OUT),
    ],
    &[
        stop(Spot::Stall(Stall::Skills, -0.2), 8.0, Doing::Read),
        by(180.0, OUT),
        stop(Spot::Stall(Stall::Plugins, -0.5), 6.0, Doing::Gaze),
        by(120.0, OUT),
        stop(Spot::Stall(Stall::Community, 0.0), 6.0, Doing::Idle),
        by(120.0, OUT),
        by(150.0, OUT + 0.3),
        by(180.0, OUT),
    ],
    // One who lingers over tea.
    &[
        stop(Spot::Tea, 24.0, Doing::Idle),
        by(60.0, OUT),
        stop(Spot::Stall(Stall::Models, -0.1), 6.0, Doing::Idle),
        by(60.0, OUT),
    ],
    &[
        stop(Spot::Stall(Stall::Plugins, 0.1), 9.0, Doing::Gaze),
        by(180.0, OUT),
        stop(Spot::Stall(Stall::Skills, -0.6), 6.0, Doing::Idle),
        by(180.0, OUT),
    ],
    &[
        stop(Spot::Stall(Stall::Community, -0.5), 9.0, Doing::Idle),
        by(60.0, OUT),
        stop(Spot::Stall(Stall::Models, -0.6), 6.0, Doing::Idle),
        by(60.0, OUT),
        stop(Spot::Tea, 7.0, Doing::Idle),
        by(60.0, OUT),
    ],
    // One who goes to look at the camel.
    &[
        stop(Spot::Stall(Stall::Hamlets, 0.2), 6.0, Doing::Idle),
        by(240.0, OUT),
        stop(Spot::Stall(Stall::Skills, -0.3), 7.0, Doing::Idle),
        by(180.0, OUT),
        stop(Spot::Stall(Stall::Plugins, -0.6), 5.0, Doing::Idle),
        by(180.0, OUT),
        by(210.0, OUT + 0.3),
        by(240.0, OUT),
        stop(Spot::At(-7.79, -4.5, 1.047), 6.0, Doing::Idle),
        by(240.0, OUT),
    ],
    &[
        stop(Spot::Stall(Stall::Models, 0.1), 6.0, Doing::Idle),
        by(0.0, GATE),
        stop(Spot::Stall(Stall::Assets, -0.6), 8.0, Doing::Gaze),
        by(0.0, GATE),
        stop(Spot::At(-0.9, 5.4, PI), 5.0, Doing::Idle),
        by(0.0, GATE),
    ],
    // One who stands long at the merchant's counter.
    &[
        stop(Spot::At(0.9, 3.95, PI), 10.0, Doing::Idle),
        by(0.0, GATE),
        stop(Spot::Stall(Stall::Models, 0.3), 5.0, Doing::Idle),
        by(0.0, GATE),
        stop(Spot::Stall(Stall::Assets, 0.3), 6.0, Doing::Idle),
        by(0.0, GATE),
    ],
];

/// `from` turned toward `to` by the share `t`, the short way round.
fn turned(from: f32, to: f32, t: f32) -> f32 {
    let gap = (to - from + PI).rem_euclid(2.0 * PI) - PI;
    from + gap * t.clamp(0.0, 1.0)
}

/// The way a walk from `a` to `b` faces.
fn heading(a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    d.x.atan2(d.y)
}

/// Where a shopper on `round` is `t` seconds in, walking at `pace`
/// and turning in `turn` seconds: its spot, the way it faces and
/// what it does.
fn whereabouts(
    round: &[Stop],
    t: f32,
    (pace, turn): (f32, f32),
) -> (Vec2, f32, Doing) {
    let next = |i: usize| round[(i + 1) % round.len()];
    let prev = |i: usize| round[(i + round.len() - 1) % round.len()];
    let walk = |i: usize| round[i].at().distance(next(i).at()) / pace;
    let came = |i: usize| heading(prev(i).at(), round[i].at());
    let long = (0..round.len())
        .map(|i| round[i].stays + walk(i))
        .sum::<f32>();
    let mut t = t.rem_euclid(long);
    for (i, here) in round.iter().enumerate() {
        let (at, facing) = here.place();
        let facing = facing.unwrap_or_else(|| came(i));
        if t < here.stays {
            return (
                at,
                turned(came(i), facing, t / turn),
                here.doing,
            );
        }
        t -= here.stays;
        let going = walk(i);
        if t < going {
            let way = heading(at, next(i).at());
            // One that only passed this stop came in walking.
            let from = match here.stays > 0.0 {
                true => facing,
                false => came(i),
            };
            let at = at.lerp(next(i).at(), t / going);
            return (at, turned(from, way, t / turn), Doing::Walk);
        }
        t -= going;
    }
    let (at, facing) = round[0].place();
    (at, facing.unwrap_or(0.0), round[0].doing)
}

/// Moves each shopper along its round and gives it its stance, and
/// keeps frames coming while one walks.
pub(super) fn stroll(
    clock: Res<Clock>,
    chief: Res<ChiefSettings>,
    set: Res<BazaarSettings>,
    mut shoppers: Query<(&Shopper, &mut Transform, &mut Stance)>,
    mut redraw: MessageWriter<bevy::window::RequestRedraw>,
) {
    let mut walking = false;
    for (shopper, mut at, mut stance) in &mut shoppers {
        let round = shopper.round % ROUNDS.len();
        let t = clock.t + AHEAD[round];
        let (spot, facing, doing) = whereabouts(
            ROUNDS[round],
            t,
            (set.pace, set.shopper_turn),
        );
        walking |= doing == Doing::Walk;
        let to = Transform::from_xyz(spot.x, FLOOR, spot.y)
            .with_rotation(Quat::from_rotation_y(facing));
        if *at != to {
            *at = to;
        }
        stance.set_if_neq(Stance {
            pose: Pose::of(doing, t),
            size: chief.size * set.grown,
            crest: mascot::crest(LOOKS[round].hat),
        });
    }
    if walking {
        redraw.write(bevy::window::RequestRedraw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bazaar::build::{KEEPER, PALMS, POSTS};
    use crate::bazaar::merchant::STANDS;

    /// The pace and the turn shoppers walk at, as designed.
    fn gait() -> (f32, f32) {
        let set = BazaarSettings::default();
        (set.pace, set.shopper_turn)
    }

    /// The length of a round at `pace`, stays and walks.
    fn long(round: &[Stop], pace: f32) -> f32 {
        (0..round.len())
            .map(|i| {
                round[i].stays
                    + round[i]
                        .at()
                        .distance(round[(i + 1) % round.len()].at())
                        / pace
            })
            .sum::<f32>()
    }

    #[test]
    fn a_shopper_walks_between_its_stops_and_faces_each_stall() {
        let gait = gait();
        let round = ROUNDS[0];
        let (first, second) = (round[0].at(), round[1].at());
        // At the first stop, turned to its stall once it has turned.
        let (at, facing, doing) =
            whereabouts(round, round[0].stays - 0.1, gait);
        assert_eq!(at, first);
        let (stall, _) = placing(Stall::Hamlets);
        let toward = heading(first, stall.xz());
        let off = (facing - toward + PI).rem_euclid(2.0 * PI) - PI;
        assert!(off.abs() < 0.2, "{facing} {toward}");
        assert_eq!(doing, Doing::Idle);
        // Halfway to the next, walking toward it.
        let walk = first.distance(second) / gait.0;
        let (at, facing, doing) =
            whereabouts(round, round[0].stays + walk * 0.5, gait);
        assert!(at.distance(first.lerp(second, 0.5)) < 1e-4);
        assert!((facing - heading(first, second)).abs() < 1e-4);
        assert_eq!(doing, Doing::Walk);
        // A round comes back to where it began.
        assert_eq!(
            whereabouts(round, long(round, gait.0) + 0.2, gait).0,
            first
        );
        // Turning goes the short way.
        assert!(
            (turned(3.0, -3.0, 1.0) - (2.0 * PI - 3.0)).abs() < 1e-4
        );
    }

    #[test]
    fn no_shopper_walks_through_what_stands_in_the_bazaar() {
        // A shopper's own room about it.
        let room = 0.25;
        let clear = |p: Vec2| {
            let pavilion = p.length() > POSTS + 0.1 + room;
            let merchant = p.distance(STANDS.xz()) > 0.3 + room;
            let tea = p.distance(TEA.0.xz()) > 0.55 + room;
            let palms = PALMS
                .iter()
                .all(|(at, _)| p.distance(at.xz()) > 0.35 + room);
            // A stall, its keeper and the work at its side, from its
            // own middle.
            let stalls = Stall::ALL.into_iter().all(|stall| {
                let (at, turn) = placing(stall);
                let local =
                    turn.inverse() * (p.extend(0.0).xzy() - at);
                local.x.abs() > 2.05 + room
                    || local.z > KEEPER.z + 0.15 + room
                    || local.z < -1.1 - room
            });
            pavilion && merchant && tea && palms && stalls
        };
        let gait = gait();
        for (k, round) in ROUNDS.iter().enumerate() {
            let step = 0.05;
            let mut t = 0.0;
            let long = long(round, gait.0);
            while t < long {
                let (at, ..) = whereabouts(round, t, gait);
                assert!(clear(at), "shopper {k} at {at} after {t}s");
                t += step;
            }
        }
    }
}
