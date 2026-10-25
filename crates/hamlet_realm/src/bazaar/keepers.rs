//! The keepers of the stalls, each on the rig of a chief's mascot in
//! a look of its own, who keep to their trades and wave at a chief
//! who comes to their stalls.

mod dress;

use std::f32::consts::{PI, TAU};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::RequestRedraw;

use super::build::{KEEPER, at_work};
use super::pitch::Making;
use super::{BazaarSettings, Stall};
use crate::Clock;
use crate::chief::mascot::{self, Doing, Pose, Stance};
use crate::chief::{
    ChiefSettings, Cloak, Hat, Holds, Light, Look, Mascot, Visitor,
};
use crate::meshes::smoothstep;
use crate::pick::{Selected, Thing};

/// Each keeper's dress, in the order of [`Stall::ALL`].
const LOOKS: [Look; 5] = [
    Look {
        cloak: Cloak::Plum,
        light: Light::ALL[1],
        hat: Hat::Bare,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Ember,
        light: Light::ALL[0],
        hat: Hat::Bare,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Slate,
        light: Light::ALL[3],
        hat: Hat::Bare,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Moss,
        light: Light::ALL[4],
        hat: Hat::Hood,
        holds: Holds::Quill,
    },
    Look {
        cloak: Cloak::Rose,
        light: Light::ALL[2],
        hat: Hat::Bare,
        holds: Holds::Book,
    },
];

/// The way a keeper faces when it is not at its work: out over the
/// plaza, a little toward its counter.
const REST: f32 = -0.3;

/// The keeper of a stall.
#[derive(Component, Clone, Copy, Debug)]
pub struct Stallholder {
    pub stall: Stall,
    /// The booth it keeps.
    booth: Entity,
    /// The clock time its stall was chosen, while it is.
    called: Option<f32>,
    /// The way it faces, as a rotation about Y from +Z.
    facing: f32,
    /// What it does, the pose it was last drawn in, the pose it
    /// eases from and the clock time that began.
    doing: Doing,
    shown: Pose,
    from: Pose,
    changed: f32,
    /// The clock time it last moved.
    seen: f32,
}

/// What the keeper of `stall` does at its own clock time `t` while
/// nobody comes to it, and whether it is at its work.
fn habit(stall: Stall, t: f32) -> (Doing, bool) {
    let (long, working, work, rest) = match stall {
        // Gazes into its great orb, then cries its minds.
        Stall::Models => (15.0, 7.0, Doing::Gaze, Doing::Attend),
        // Bends to the robot it builds, then stands back.
        Stall::Community => (16.0, 10.0, Doing::Tend, Doing::Idle),
        // Works its grindstone, then looks about.
        Stall::Plugins => (14.0, 9.0, Doing::Tend, Doing::Idle),
        // Reads at its lectern, then looks about.
        Stall::Skills => (16.0, 10.0, Doing::Read, Doing::Idle),
        // Talks up its land, then reads its ledger.
        Stall::Hamlets => (14.0, 7.0, Doing::Read, Doing::Attend),
    };
    match t.rem_euclid(long) < working {
        true => (work, true),
        false => (rest, false),
    }
}

/// The keeper's own clock: each a few seconds apart, so that no two
/// change together.
fn own(stall: Stall, t: f32) -> f32 {
    t + stall as usize as f32 * 3.7
}

/// True when the keeper of `stall` is at its work at clock time `t`.
pub(super) fn working(stall: Stall, t: f32) -> bool {
    habit(stall, own(stall, t)).1
}

/// `from` turned toward `to` by no more than `most`, the short way
/// round.
fn turned(from: f32, to: f32, most: f32) -> f32 {
    let gap = (to - from + PI).rem_euclid(TAU) - PI;
    from + gap.clamp(-most, most)
}

/// Stands the keeper of `stall` in `small` of `booth`, in its dress
/// and with what it wears and holds for its trade.
pub(super) fn stand(
    commands: &mut Commands,
    making: &mut Making,
    (booth, small): (Entity, Entity),
    stall: Stall,
) {
    let (keeper, rig) =
        making.figure(commands, small, LOOKS[stall as usize], KEEPER);
    let pose = Pose::of(Doing::Idle, 0.0);
    commands.entity(keeper).insert((
        Stallholder {
            stall,
            booth,
            called: None,
            facing: REST,
            doing: Doing::Idle,
            shown: pose,
            from: pose,
            changed: f32::MIN,
            seen: 0.0,
        },
        rig,
    ));
    let worn = dress::worn(making.pal, stall);
    for (part, mesh, lit) in [
        (rig.head, worn.head, false),
        (rig.item, worn.held, false),
        (rig.item, worn.glows, true),
    ] {
        if mesh.is_empty() {
            continue;
        }
        let material = match lit {
            true => making.gleam.clone(),
            false => making.stock.matte.clone(),
        };
        let mut part = commands.spawn((
            Mesh3d(making.meshes.add(mesh.build())),
            MeshMaterial3d(material),
            ChildOf(part),
        ));
        if lit {
            part.insert(NotShadowCaster);
        }
    }
}

/// Has each keeper keep to its trade, or turn to a chief who comes
/// to its stall and wave, then listen. The same system that places a
/// chief's mascot places it.
pub(super) fn tend(
    clock: Res<Clock>,
    chief: Res<ChiefSettings>,
    set: Res<BazaarSettings>,
    selected: Res<Selected>,
    booths: Query<(&Visitor, &GlobalTransform)>,
    mascots: Query<&GlobalTransform, With<Mascot>>,
    mut keepers: Query<(
        &mut Stallholder,
        &mut Transform,
        &mut Stance,
    )>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let now = clock.t;
    let mut moving = false;
    for (mut keeper, mut at, mut stance) in &mut keepers {
        let stall = keeper.stall;
        let called = match selected.0 == Some(Thing::Stall(stall)) {
            true => keeper.called.or(Some(now)),
            false => None,
        };
        keeper.called = called;
        let t = own(stall, now);
        let (doing, yaw) = match called {
            Some(since) => {
                // The chief's mascot when it has come, or the spot
                // it will stand on.
                let them = booths
                    .get(keeper.booth)
                    .map(|(visitor, place)| {
                        let inward = place.affine().inverse();
                        mascots
                            .iter()
                            .map(|m| {
                                inward
                                    .transform_point3(m.translation())
                            })
                            .filter(|p| {
                                p.xz().distance(visitor.0.xz())
                                    < set.near
                            })
                            .min_by(|a, b| {
                                a.distance(visitor.0)
                                    .total_cmp(&b.distance(visitor.0))
                            })
                            .unwrap_or(visitor.0)
                    })
                    .unwrap_or(KEEPER + Vec3::Z);
                let way = them - KEEPER;
                let doing = match now - since < set.waves {
                    true => Doing::Wave,
                    false => Doing::Attend,
                };
                (doing, way.x.atan2(way.z))
            }
            None => {
                let (doing, busy) = habit(stall, t);
                (doing, if busy { at_work() } else { REST })
            }
        };
        let dt = (now - keeper.seen).clamp(0.0, 0.1);
        keeper.seen = now;
        let facing = turned(keeper.facing, yaw, set.turn * dt);
        moving |= (facing - keeper.facing).abs() > 1e-4
            || doing == Doing::Wave;
        keeper.facing = facing;
        if keeper.doing != doing {
            keeper.from = keeper.shown;
            keeper.doing = doing;
            keeper.changed = now;
        }
        let eased = smoothstep(0.0, set.blend, now - keeper.changed);
        let pose = keeper.from.toward(&Pose::of(doing, t), eased);
        moving |= eased < 1.0;
        keeper.shown = pose;
        let to = Transform::from_translation(KEEPER)
            .with_rotation(Quat::from_rotation_y(facing));
        if *at != to {
            *at = to;
        }
        stance.set_if_neq(Stance {
            pose,
            size: chief.size,
            crest: mascot::crest(LOOKS[stall as usize].hat),
        });
    }
    if moving {
        redraw.write(RequestRedraw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keeper_turns_the_short_way_and_no_further_than_it_may() {
        // From just short of a half turn one way to just past it the
        // other: the short way is across the back.
        let from = PI - 0.1;
        let to = -PI + 0.1;
        let step = turned(from, to, 0.05);
        assert!((step - (from + 0.05)).abs() < 1e-5, "{step}");
        let there = turned(from, to, 1.0);
        assert!((there - (from + 0.2)).abs() < 1e-5, "{there}");
    }
}
