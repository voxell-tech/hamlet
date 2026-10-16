//! The shoppers of the bazaar, who go from stall to stall and look
//! at what is for sale.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;
use bevy::window::RequestRedraw;

use super::Bazaar;
use super::build::FLOOR;
use crate::Clock;
use crate::chief::mascot::{self, Doing, Pose, Stance};
use crate::chief::{ChiefSettings, Cloak, Hat, Holds, Light, Look};

/// A shopper: no chief and no agent, only one who goes its round of
/// the bazaar.
#[derive(Component, Clone, Copy, Debug)]
pub struct Shopper {
    /// Its round, by its place in [`ROUNDS`].
    pub round: usize,
}

/// A stop on a shopper's round: where it stands, the way it faces
/// there, the seconds it stays and what it does meanwhile.
#[derive(Clone, Copy, Debug)]
pub(super) struct Stop {
    at: Vec2,
    facing: f32,
    stays: f32,
    doing: Doing,
}

const fn stop(
    x: f32,
    z: f32,
    facing: f32,
    stays: f32,
    doing: Doing,
) -> Stop {
    Stop {
        at: Vec2::new(x, z),
        facing,
        stays,
        doing,
    }
}

/// Each shopper's dress.
pub(super) const LOOKS: [Look; 3] = [
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
];

/// Each shopper's round, from the bazaar's middle. A stop it stays
/// at for no time is one it only passes, to keep clear of the
/// pavilion's posts.
pub(super) const ROUNDS: [&[Stop]; 3] = [
    &[
        stop(-1.0, 5.25, -1.9, 4.0, Doing::Idle),
        stop(-3.3, 2.3, -FRAC_PI_2, 5.0, Doing::Idle),
        stop(-3.45, -2.55, -FRAC_PI_2, 6.0, Doing::Gaze),
        stop(-3.4, 2.9, 0.0, 0.0, Doing::Walk),
    ],
    &[
        stop(3.3, 2.3, FRAC_PI_2, 5.0, Doing::Idle),
        stop(3.45, -2.55, FRAC_PI_2, 7.0, Doing::Read),
        stop(3.4, 2.9, 0.0, 0.0, Doing::Walk),
        stop(3.69, 3.61, 0.61, 6.0, Doing::Gaze),
    ],
    &[
        stop(0.85, 2.55, PI, 6.0, Doing::Idle),
        stop(1.2, 4.05, 2.39, 7.0, Doing::Idle),
        stop(-0.3, 5.0, 0.0, 4.0, Doing::Gaze),
    ],
];

/// A shopper's walk, in units a second, and the seconds it takes to
/// turn.
const PACE: f32 = 0.85;
const TURN: f32 = 0.45;

/// A shopper's size as a share of a chief's mascot.
pub(super) const GROWN: f32 = 0.82;

/// The seconds each shopper's round is ahead of the first's, so that
/// they are seldom all still at once.
const AHEAD: [f32; 3] = [0.0, 7.5, 3.2];

/// `to` turned toward `from` by the share `t`, the short way round.
fn turned(from: f32, to: f32, t: f32) -> f32 {
    let gap = (to - from + PI).rem_euclid(2.0 * PI) - PI;
    from + gap * t.clamp(0.0, 1.0)
}

/// The way a walk from `a` to `b` faces.
fn heading(a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    d.x.atan2(d.y)
}

/// Where a shopper on `round` is `t` seconds in: its spot, the way
/// it faces and what it does.
pub(super) fn whereabouts(
    round: &[Stop],
    t: f32,
) -> (Vec2, f32, Doing) {
    let next = |i: usize| round[(i + 1) % round.len()];
    let prev = |i: usize| round[(i + round.len() - 1) % round.len()];
    let walk = |i: usize| round[i].at.distance(next(i).at) / PACE;
    let long = (0..round.len())
        .map(|i| round[i].stays + walk(i))
        .sum::<f32>();
    let mut t = t.rem_euclid(long);
    for (i, here) in round.iter().enumerate() {
        if t < here.stays {
            let came = heading(prev(i).at, here.at);
            return (
                here.at,
                turned(came, here.facing, t / TURN),
                here.doing,
            );
        }
        t -= here.stays;
        let going = walk(i);
        if t < going {
            let way = heading(here.at, next(i).at);
            // One that only passed this stop came in walking.
            let from = match here.stays > 0.0 {
                true => here.facing,
                false => heading(prev(i).at, here.at),
            };
            let at = here.at.lerp(next(i).at, t / going);
            return (at, turned(from, way, t / TURN), Doing::Walk);
        }
        t -= going;
    }
    (round[0].at, round[0].facing, round[0].doing)
}

/// Moves each shopper along its round and gives it its stance, and
/// keeps frames coming while one walks where it can be seen.
pub(super) fn stroll(
    clock: Res<Clock>,
    set: Res<ChiefSettings>,
    mut shoppers: Query<(&Shopper, &mut Transform, &mut Stance)>,
    bazaars: Query<&ViewVisibility, With<Bazaar>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let mut walking = false;
    for (shopper, mut at, mut stance) in &mut shoppers {
        let round = shopper.round % ROUNDS.len();
        let t = clock.t + AHEAD[round];
        let (spot, facing, doing) = whereabouts(ROUNDS[round], t);
        walking |= doing == Doing::Walk;
        let to = Transform::from_xyz(spot.x, FLOOR, spot.y)
            .with_rotation(Quat::from_rotation_y(facing));
        if *at != to {
            *at = to;
        }
        stance.set_if_neq(Stance {
            pose: Pose::of(doing, t),
            size: set.size * GROWN,
            crest: mascot::crest(LOOKS[round].hat),
        });
    }
    if walking && bazaars.iter().any(|seen| seen.get()) {
        redraw.write(RequestRedraw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shopper_walks_between_its_stops_and_faces_each_stall() {
        let round = ROUNDS[0];
        // At the first stop, turned to its stall once it has turned.
        let (at, facing, doing) =
            whereabouts(round, round[0].stays - 0.1);
        assert_eq!(at, round[0].at);
        assert!((facing - round[0].facing).abs() < 1e-4);
        assert_eq!(doing, Doing::Idle);
        // Halfway to the next, walking toward it.
        let walk = round[0].at.distance(round[1].at) / PACE;
        let (at, facing, doing) =
            whereabouts(round, round[0].stays + walk * 0.5);
        assert!(
            at.distance(round[0].at.lerp(round[1].at, 0.5)) < 1e-4
        );
        assert!(
            (facing - heading(round[0].at, round[1].at)).abs() < 1e-4
        );
        assert_eq!(doing, Doing::Walk);
        // A round comes back to where it began.
        let long = (0..round.len())
            .map(|i| {
                round[i].stays
                    + round[i]
                        .at
                        .distance(round[(i + 1) % round.len()].at)
                        / PACE
            })
            .sum::<f32>();
        assert_eq!(whereabouts(round, long + 0.2).0, round[0].at);
        // Turning goes the short way.
        assert!(
            (turned(3.0, -3.0, 1.0) - (2.0 * PI - 3.0)).abs() < 1e-4
        );
    }
}
