//! Wickstep: the mascot goes out like a candle where it stands and is
//! lit again somewhere else. Over a short way it walks.

mod charm;

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_vfx::magic::chance;
use hamlet_vfx::mark::{MarkMaterial, Marks};

#[cfg(test)]
pub(super) use self::charm::{Mote, Rune};
use self::charm::{Wand, charm};
pub(super) use self::charm::{cast, prepare};
use super::mascot::{Doing, Pose};
use super::place::{self, Pastime};
use super::{Beside, ChiefSettings, Look, Place, Settled};
use crate::Clock;
use crate::bazaar::{self, Bazaar};
use crate::generator::land::Land;
use crate::generator::{
    Common, Floor, GenView, House, Passage, Robot, homing,
};
use crate::hamlet::{Housed, Sited};
use crate::hand::PlaySettings;
use crate::keeper::Act;
use crate::library::{self, Library};
use crate::palette::Palette;
use crate::pick::{Hovered, Thing};
use crate::play::{Play, Played};
use crate::sky::Quality;

/// Farthest the spot a mascot walks to may move before its way is
/// found again.
const REPLAN: f32 = 0.5;

/// The part of its course a Wickstep is in. `long` is the seconds
/// the part takes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phase {
    /// The mascot stands where it is.
    Lit,
    Out {
        began: f32,
        long: f32,
    },
    /// Gone from one place and not yet at the other.
    Dark {
        began: f32,
        long: f32,
    },
    In {
        began: f32,
        long: f32,
    },
}

/// A mascot's whereabouts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stay {
    Home,
    /// Beside this agent.
    With(Entity),
}

/// A chief's mascot.
#[derive(Component, Debug)]
pub struct Mascot {
    pub chief: Entity,
    /// Its place, or the place it is leaving.
    pub stay: Stay,
    pub phase: Phase,
    pub doing: Doing,
    /// The pose it was last drawn in, the pose it eases from and
    /// the clock time that began.
    pub(super) shown: Pose,
    pub(super) from: Pose,
    pub(super) changed: f32,
    /// The light of its wick, which its motes share.
    pub(super) flame: Handle<StandardMaterial>,
    /// The way it faces, as a rotation about Y from +Z.
    yaw: f32,
    /// Its offset from its agent.
    side: Vec2,
    /// The way it walks after its agent, as far as where it was
    /// going when the way was found.
    way: Vec<Vec2>,
    /// The spot of its grounds it is at or walking to, and the
    /// clock time it moves on.
    spot: usize,
    until: f32,
    /// The clock time it last moved.
    seen: f32,
    /// Counts its Wicksteps and its choices of a spot.
    turn: u32,
}

impl Mascot {
    /// A mascot at home, facing `yaw`, that stays where it is until
    /// the clock time `until`.
    pub(super) fn new(
        chief: Entity,
        flame: Handle<StandardMaterial>,
        yaw: f32,
        now: f32,
        until: f32,
    ) -> Self {
        let pose = Pose::of(Doing::Idle, now);
        Self {
            chief,
            stay: Stay::Home,
            phase: Phase::Lit,
            doing: Doing::Idle,
            shown: pose,
            from: pose,
            changed: f32::MIN,
            flame,
            yaw,
            side: Vec2::ZERO,
            way: Vec::new(),
            spot: 0,
            until,
            seen: now,
            turn: 0,
        }
    }

    /// Takes up `doing`, easing into it from the pose it is in.
    fn act(&mut self, doing: Doing, now: f32) {
        if self.doing != doing {
            self.doing = doing;
            self.from = self.shown;
            self.changed = now;
        }
    }

    /// True when it is in the middle of a Wickstep.
    pub(crate) fn stepping(&self) -> bool {
        self.phase != Phase::Lit
    }
}

/// The rotation about Y that turns +Z to face along `way`.
fn facing(way: Vec2) -> f32 {
    way.x.atan2(way.y)
}

/// `from` turned toward `to` by no more than `most`, the short way
/// round.
fn turned(from: f32, to: f32, most: f32) -> f32 {
    let gap = (to - from + PI).rem_euclid(TAU) - PI;
    from + gap.clamp(-most, most)
}

/// The next point for a mascot on foot: the next of the rest of its
/// `way`, and straight to `target` from its last. A way it has walked
/// to its end is no reason for a Wickstep: only one that was never
/// found is, which `dry` tells.
fn leg(way: &[Vec2], target: Vec2, dry: bool) -> Option<Vec2> {
    match way {
        [] => dry.then_some(target),
        [_] => Some(target),
        [next, ..] => Some(*next),
    }
}

/// The place of the agent of the entity `agent`: its robot, or the
/// front of its house while it has none.
fn whereabouts(
    agent: Entity,
    agents: &Query<&Housed>,
    robots: &Query<(&Robot, &GlobalTransform)>,
    houses: &Query<(&House, &GlobalTransform)>,
) -> Option<Vec3> {
    let id = agents.get(agent).ok()?.0;
    robots
        .iter()
        .find(|(robot, _)| robot.agent == id)
        .map(|(_, at)| at.translation())
        .or_else(|| {
            houses
                .iter()
                .find(|(house, _)| {
                    house.agent == id && !house.archived
                })
                .map(|(_, at)| at.transform_point(Vec3::Z * 3.2))
        })
}

/// The middle of the common of the hamlet `hamlet`. Nothing for an
/// entity that is no hamlet, or one whose common is not built.
fn common_of(
    hamlet: Entity,
    sites: &Query<&Sited>,
    commons: &Query<(&Common, &GlobalTransform), Without<Mascot>>,
) -> Option<Vec2> {
    let town = sites.get(hamlet).ok()?.0;
    commons
        .iter()
        .find(|(common, _)| common.town == town)
        .map(|(_, at)| at.translation().xz())
}

/// Moves each mascot: a Wickstep to whoever its chief talks to and
/// home again, and on foot between.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn step(
    mut commands: Commands,
    (clock, set, play, quality, pal, land, wand): (
        Res<Clock>,
        Res<ChiefSettings>,
        Res<PlaySettings>,
        Res<Quality>,
        Res<Palette>,
        Res<Land>,
        Res<Wand>,
    ),
    view: Option<Res<GenView>>,
    hovered: Option<Res<Hovered>>,
    chiefs: Query<(&Beside, &Look, &Settled)>,
    places: Query<&Transform, (With<Place>, Without<Mascot>)>,
    (agents, sites, commons, libraries): (
        Query<&Housed>,
        Query<&Sited>,
        Query<(&Common, &GlobalTransform), Without<Mascot>>,
        Query<
            (&GlobalTransform, Has<Bazaar>),
            Or<(With<Library>, With<Bazaar>)>,
        >,
    ),
    robots: Query<(&Robot, &GlobalTransform)>,
    houses: Query<(&House, &GlobalTransform)>,
    free: Query<(Entity, &Robot), Without<Passage>>,
    mut mascots: Query<(&mut Mascot, &mut Transform)>,
    mut materials: ResMut<Assets<MarkMaterial>>,
    marks: Res<Marks>,
    mut floor: Floor,
    mut played: MessageWriter<Played>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let now = clock.t;
    // Toward the camera and to its right, along the ground.
    let yaw = view.map_or(0.0, |view| view.yaw);
    let (back, right) = (
        Vec2::new(yaw.sin(), yaw.cos()),
        Vec2::new(yaw.cos(), -yaw.sin()),
    );
    let spots = place::spots(&set);
    for (mut mascot, mut at) in &mut mascots {
        let Ok((beside, look, settled)) = chiefs.get(mascot.chief)
        else {
            continue;
        };
        let Ok(home) = places.get(settled.place) else {
            continue;
        };
        let dt = (now - mascot.seen).clamp(0.0, 0.1);
        mascot.seen = now;
        let on_ground = |p: Vec2| land.on_ground(p);
        let spot_at = |i: usize| {
            let local = spots[i].at;
            home.transform_point(Vec3::new(local.x, 0.0, local.y))
                .xz()
        };
        let round =
            |hamlet: Entity| common_of(hamlet, &sites, &commons);
        // By a common it stands at one spot around it and then
        // another, as one who looks the place over.
        let of = |agent: Entity| match round(agent) {
            Some(middle) => {
                let (reach, pause) = set.stroll;
                let turn = (now / pause).floor();
                let seed = (agent.to_bits() % 97) as f32;
                // Each spot is a short walk on from the last, so
                // that it goes round the well and never across.
                let angle =
                    seed + turn * 0.9 + chance(turn * 1.3) * 0.2;
                let spot = middle + Vec2::from_angle(angle) * reach;
                Some(Vec3::new(spot.x, 0.0, spot.y))
            }
            // By the library it stands at the foot of its steps,
            // and in the bazaar's avenue.
            None => libraries
                .get(agent)
                .map(|(at, bazaar)| {
                    let visitor = match bazaar {
                        true => bazaar::VISITOR,
                        false => library::VISITOR,
                    };
                    at.transform_point(visitor)
                })
                .ok()
                .or_else(|| {
                    whereabouts(agent, &agents, &robots, &houses)
                }),
        };
        // An agent that is nowhere to be found is not stood by.
        let wanted = match beside.0.filter(|a| of(*a).is_some()) {
            Some(agent) => Stay::With(agent),
            None => Stay::Home,
        };
        let light = look.light.colour(&pal);
        let here = at.translation;

        // Sets it down where `wanted` has it.
        let land_at = |mascot: &mut Mascot, at: &mut Transform| {
            mascot.stay = wanted;
            mascot.way.clear();
            mascot.yaw = match wanted {
                Stay::With(agent) => {
                    let them = of(agent).unwrap_or(here).xz();
                    let side = right * set.beside + back * set.before;
                    let stand =
                        homing::beside(&land, them, side, set.restep)
                            .unwrap_or(them + side);
                    mascot.side = stand - them;
                    at.translation = on_ground(stand);
                    facing(back * set.cheat - mascot.side)
                }
                Stay::Home => {
                    mascot.spot = 0;
                    let linger = set.linger.0 * 0.5;
                    mascot.until = now + linger;
                    at.translation = on_ground(spot_at(0));
                    let out = home.rotation * Vec3::Z;
                    facing(out.xz())
                }
            };
            at.rotation = Quat::from_rotation_y(mascot.yaw);
        };

        match mascot.phase {
            Phase::Lit if wanted != mascot.stay && clock.frozen => {
                // A still shows it there, with no magic in the air.
                land_at(&mut mascot, &mut at);
            }
            Phase::Lit if wanted != mascot.stay => {
                // An agent near enough is walked to, as one in the
                // same town is: a Wickstep is for a long way.
                let near = match wanted {
                    Stay::With(agent) => of(agent)
                        .map(|them| them.xz())
                        .filter(|them| {
                            them.distance(here.xz()) <= set.afoot
                        }),
                    Stay::Home => None,
                };
                match near {
                    Some(them) => {
                        let side =
                            right * set.beside + back * set.before;
                        let stand = homing::beside(
                            &land, them, side, set.restep,
                        )
                        .unwrap_or(them + side);
                        mascot.side = stand - them;
                        mascot.stay = wanted;
                        mascot.way.clear();
                    }
                    None => {
                        mascot.phase = Phase::Out {
                            began: now,
                            long: set.out_time,
                        };
                    }
                }
            }
            Phase::Lit => {
                let waved = hovered.as_ref().is_some_and(|h| {
                    h.0 == Some(Thing::Chief(mascot.chief))
                });
                let (target, rest, look_at) = match mascot.stay {
                    Stay::With(agent) => {
                        let them = of(agent).unwrap_or(here).xz();
                        // Its agent has gone where its own spot is
                        // wet or taken.
                        if !homing::clear(&land, them + mascot.side) {
                            let side = mascot
                                .side
                                .normalize_or(right)
                                * Vec2::new(set.beside, set.before)
                                    .length();
                            if let Some(stand) = homing::beside(
                                &land, them, side, set.restep,
                            ) {
                                mascot.side = stand - them;
                            }
                        }
                        match round(agent) {
                            // It looks at the common.
                            Some(middle) => (
                                them + mascot.side,
                                Doing::Gaze,
                                middle,
                            ),
                            None => (
                                them + mascot.side,
                                Doing::Attend,
                                them + back * set.cheat,
                            ),
                        }
                    }
                    Stay::Home => {
                        if now >= mascot.until {
                            // On to another spot. It lingers there
                            // from when it arrives.
                            mascot.turn = mascot.turn.wrapping_add(1);
                            let roll =
                                chance(mascot.turn as f32 * 1.7);
                            let others = spots.len() - 1;
                            let hop = 1
                                + (roll * others as f32) as usize
                                    % others;
                            mascot.spot =
                                (mascot.spot + hop) % spots.len();
                            mascot.until = f32::MAX;
                        }
                        let spot = spots[mascot.spot];
                        let rest = match spot.pastime {
                            Pastime::Wait => Doing::Idle,
                            Pastime::Read => Doing::Read,
                            Pastime::Tend => Doing::Tend,
                            Pastime::Gaze => Doing::Gaze,
                        };
                        let faces = home
                            .transform_point(Vec3::new(
                                spot.faces.x,
                                0.0,
                                spot.faces.y,
                            ))
                            .xz();
                        (spot_at(mascot.spot), rest, faces)
                    }
                };
                let gap = target - here.xz();
                let far = gap.length();
                let walking = mascot.doing == Doing::Walk;
                // Beside an agent it lets it stray a little before
                // it follows.
                let slack = match mascot.stay {
                    Stay::With(_) if !walking => set.slack,
                    _ => 0.05,
                };
                let mut way = look_at - here.xz();
                if matches!(mascot.stay, Stay::With(_))
                    && far > set.afoot
                {
                    // Too far to walk: it steps after its agent.
                    mascot.phase = Phase::Out {
                        began: now,
                        long: set.out_time,
                    };
                } else if far > slack {
                    let next = match mascot.stay {
                        Stay::With(_) => {
                            let found = mascot
                                .way
                                .last()
                                .is_some_and(|end| {
                                    end.distance(target) < REPLAN
                                });
                            // True when there is a dry way on foot:
                            // the one it is on, or one found now.
                            let mut dry = found;
                            if !found {
                                let way = homing::overland(
                                    &land,
                                    here.xz(),
                                    target,
                                );
                                dry = way.is_some();
                                mascot.way = way.unwrap_or_default();
                            }
                            let passed = mascot
                                .way
                                .iter()
                                .take_while(|p| {
                                    p.distance(here.xz()) < 0.05
                                })
                                .count();
                            mascot.way.drain(..passed);
                            leg(&mascot.way, target, dry)
                        }
                        Stay::Home => Some(target),
                    };
                    if let Some(next) = next {
                        // The further its agent is ahead, the faster
                        // it goes, up to as fast as a robot does.
                        let behind = match mascot.stay {
                            Stay::With(_) => ((far - set.slack)
                                / (set.restep * 0.5 - set.slack))
                                .clamp(0.0, 1.0),
                            Stay::Home => 0.0,
                        };
                        let top =
                            play.scurry_fastest.max(set.walk_speed);
                        let speed = set.walk_speed
                            + (top - set.walk_speed) * behind;
                        let leg = next - here.xz();
                        let stride = (speed * dt).min(leg.length());
                        at.translation = on_ground(
                            here.xz()
                                + leg.normalize_or_zero() * stride,
                        );
                        way = leg;
                        mascot.act(Doing::Walk, now);
                        redraw.write(RequestRedraw);
                    } else {
                        // No dry way on foot: it steps after its
                        // agent.
                        mascot.phase = Phase::Out {
                            began: now,
                            long: set.out_time,
                        };
                    }
                } else {
                    mascot.way.clear();
                    if mascot.until == f32::MAX {
                        let (least, most) = set.linger;
                        let roll = chance(mascot.turn as f32 + 0.37);
                        mascot.until =
                            now + least + (most - least) * roll;
                    }
                    if waved {
                        way = back;
                        mascot.act(Doing::Wave, now);
                        redraw.write(RequestRedraw);
                    } else {
                        mascot.act(rest, now);
                    }
                }
                if way.length_squared() > 1e-6 {
                    mascot.yaw = turned(
                        mascot.yaw,
                        facing(way),
                        set.turn_rate * dt,
                    );
                    at.rotation = Quat::from_rotation_y(mascot.yaw);
                }
            }
            Phase::Out { began, long } if now - began >= long => {
                mascot.phase = Phase::Dark {
                    began: now,
                    long: set.dark_time,
                };
            }
            Phase::Dark { began, long } if now - began >= long => {
                let came = mascot.stay != wanted;
                land_at(&mut mascot, &mut at);
                mascot.phase = Phase::In {
                    began: now,
                    long: set.in_time,
                };
                // Its agent waves as it comes, unless it is in the
                // middle of a motion of its own.
                let greeter = match wanted {
                    Stay::With(agent) if came => {
                        agents.get(agent).ok().and_then(|housed| {
                            free.iter().find(|(_, robot)| {
                                robot.agent == housed.0
                            })
                        })
                    }
                    _ => None,
                };
                if let Some((robot, _)) = greeter {
                    commands
                        .entity(robot)
                        .try_insert(Passage::new(Act::Wave));
                }
                let rest = match wanted {
                    Stay::With(_) => Doing::Attend,
                    Stay::Home => Doing::Idle,
                };
                mascot.doing = rest;
                mascot.changed = f32::MIN;
                mascot.turn = mascot.turn.wrapping_add(1);
                charm(
                    &mut commands,
                    &wand,
                    &mut materials,
                    &marks,
                    &mut floor,
                    &set,
                    *quality,
                    &mascot,
                    light,
                    at.translation,
                    now,
                    true,
                );
                played.write(Played {
                    what: Play::WickIn,
                    at: at.translation,
                });
            }
            Phase::In { began, long } if now - began >= long => {
                mascot.phase = Phase::Lit;
            }
            Phase::Out { .. }
            | Phase::Dark { .. }
            | Phase::In { .. } => {}
        }
        // It has just begun to go out.
        if let Phase::Out { began, .. } = mascot.phase
            && began == now
        {
            charm(
                &mut commands,
                &wand,
                &mut materials,
                &marks,
                &mut floor,
                &set,
                *quality,
                &mascot,
                light,
                here,
                now,
                false,
            );
            played.write(Played {
                what: Play::WickOut,
                at: here,
            });
        }
        if mascot.stepping() {
            redraw.write(RequestRedraw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_way_walked_to_its_end_is_walked_on_from_and_not_stepped() {
        let (near, target) =
            (Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0));
        // On its way, it goes to the next point of it.
        assert_eq!(leg(&[near, target], target, true), Some(near));
        assert_eq!(leg(&[near], target, true), Some(target));
        // At the end of a way its agent has strayed a little from,
        // it walks the rest.
        assert_eq!(leg(&[], target, true), Some(target));
        // Only where no way was found does it step.
        assert_eq!(leg(&[], target, false), None);
    }
}
