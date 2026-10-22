//! The shoppers of the bazaar, who go from stall to stall, look at
//! what is for sale and linger over tea.

use std::f32::consts::PI;

use glam::{Quat, Vec2, Vec3, Vec3Swizzles};

use crate::folk::chief::look::{Cloak, Hat, Holds, Light, Look};
use crate::folk::chief::mascot::{Doing, Pose};
use crate::places::bazaar::build::{FLOOR, TEA, at_stall, placing};
use crate::places::bazaar::figure::{Figure, standing};
use crate::places::bazaar::{BazaarSettings, Stall};

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
pub const LOOKS: [Look; 8] = [
    Look {
        cloak: Cloak::Rose,
        light: Light::Rosy,
        hat: Hat::Hood,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Moss,
        light: Light::Leaf,
        hat: Hat::Wreath,
        holds: Holds::Book,
    },
    Look {
        cloak: Cloak::River,
        light: Light::Moon,
        hat: Hat::Pointed,
        holds: Holds::Teacup,
    },
    Look {
        cloak: Cloak::Cream,
        light: Light::Candle,
        hat: Hat::Crown,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Slate,
        light: Light::Rune,
        hat: Hat::Hood,
        holds: Holds::Book,
    },
    Look {
        cloak: Cloak::Ember,
        light: Light::Leaf,
        hat: Hat::Wreath,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Plum,
        light: Light::Rosy,
        hat: Hat::Pointed,
        holds: Holds::Quill,
    },
    Look {
        cloak: Cloak::Mustard,
        light: Light::Moon,
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

/// Moves shopper `round` along its round to clock time `now` and
/// gives it its pose, `size` times the size it is built at.
pub fn stroll(
    figure: &mut Figure,
    round: usize,
    set: &BazaarSettings,
    now: f32,
    size: f32,
) {
    let round = round % ROUNDS.len();
    let t = now + AHEAD[round];
    let (spot, facing, doing) =
        whereabouts(ROUNDS[round], t, (set.pace, set.shopper_turn));
    figure.place(
        standing(Vec3::new(spot.x, FLOOR, spot.y), facing),
        &Pose::of(doing, t),
        size * set.grown,
    );
}
