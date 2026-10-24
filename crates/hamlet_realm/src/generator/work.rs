//! Robots at work about their own houses (docs/working-alone.md):
//! each walks to the station of its chore and plays the chore there,
//! or is put on the road it is walking.

mod floor;
mod lot;
mod passage;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

pub use self::floor::Floor;
use self::lot::footing;
pub(super) use self::lot::{Engine, Lot};
pub use self::passage::Passage;
pub(super) use self::passage::pass;
use super::homing::houseward;
use super::land::Land;
use super::society::AgentId;
use super::sync::Drawn;
use super::traffic::Trips;
use crate::Clock;
use crate::keeper::work::{Chore, Station, Task, Work, routine};
use crate::keeper::{self, Act, Keeper, KeeperState, Motion, Stride};

/// An agent's robot. Its meshes are this entity's descendants.
#[derive(Component, Debug)]
pub struct Robot {
    pub agent: AgentId,
    /// The agent's state when it is not out on a road.
    pub(super) state: KeeperState,
    pub(super) work: Work,
    pub(super) lot: Lot,
    engine: Engine,
    /// Its doing about its house. None on a road, and before it has
    /// been placed.
    job: Option<Job>,
    /// The motion last asked of it, and the clock time it began.
    playing: Option<(Act, f32)>,
    /// The step of its agent's work last seen, and when.
    step: u32,
    step_at: f32,
    failures: u32,
    /// A failure it has yet to be startled by.
    oops: bool,
    /// A tool it has yet to be seen using.
    queued: Option<Chore>,
    /// True when its engine's rings are out.
    steaming: bool,
    /// True when it has been placed once.
    placed: bool,
    /// The speed of its next walk about its house, when it is in a
    /// hurry.
    haste: Option<f32>,
}

impl Robot {
    pub(super) fn new(
        agent: AgentId,
        state: KeeperState,
        work: Work,
        lot: Lot,
        engine: Engine,
    ) -> Self {
        Self {
            agent,
            state,
            work,
            lot,
            engine,
            job: None,
            playing: None,
            step: work.step,
            step_at: 0.0,
            failures: work.failures,
            oops: false,
            queued: None,
            steaming: false,
            placed: false,
            haste: None,
        }
    }
}

impl Robot {
    /// Every place it stands for a chore, with the way it faces
    /// there.
    pub(super) fn stations(
        &self,
        land: &Land,
    ) -> impl Iterator<Item = (Vec3, f32)> {
        [
            (Station::Porch, 0.0),
            (Station::Desk, 0.0),
            (Station::Rack, 0.0),
            (Station::Engine, 0.0),
            (Station::Open, 0.0),
            (Station::Beat, 0.0),
            (Station::Beat, 1.0),
        ]
        .into_iter()
        .filter_map(|(station, beat)| {
            self.lot.stand(land, station, beat)
        })
    }

    /// True when it is about its house, where a hand can reach it.
    pub(crate) fn is_at_house(&self) -> bool {
        self.job.is_some()
    }

    /// The spot it stands at with nothing to do.
    pub(crate) fn porch(&self) -> Vec3 {
        self.lot.porch
    }

    /// A point by where it stands with nothing to do: `by.x` to its
    /// side and `by.y` out from the house.
    pub(crate) fn by_porch(&self, by: Vec2) -> Vec3 {
        self.lot.porch
            + Quat::from_rotation_y(self.lot.yaw)
                * Vec3::new(by.x, 0.0, by.y)
    }

    /// The ground a robot stands on at `p`.
    pub fn ground(land: &Land, p: Vec2) -> f32 {
        footing(land, p)
    }

    /// Forgets the chore it was at, so that it takes its work up
    /// again from wherever it stands, at `haste` a second if given.
    pub(crate) fn put_down(&mut self, haste: Option<f32>) {
        self.job = None;
        self.playing = None;
        self.haste = haste;
    }

    /// The way it is walking and where it is bound, while it has a
    /// chore.
    pub(super) fn walk(&self) -> Option<(&[Vec3], Vec3)> {
        self.job
            .as_ref()
            .map(|job| (job.leg.way.as_slice(), job.to.0))
    }
}

/// One chore: the walk to its station, then the chore itself.
#[derive(Debug)]
struct Job {
    chore: Chore,
    /// Clock time it set off.
    since: f32,
    /// Clock time the step that asked for it began.
    began: f32,
    leg: keeper::Trip,
    /// True when its way runs on a road, whose surface it keeps to.
    by_road: bool,
    /// The spot it stands at for the chore, and the way it faces.
    to: (Vec3, f32),
}

impl Job {
    fn new(
        chore: Chore,
        since: f32,
        way: Vec<Vec3>,
        to: (Vec3, f32),
        motion: &Motion,
    ) -> Self {
        let mut leg = keeper::Trip::new(way, since, motion);
        leg.speed = motion.amble;
        Self {
            chore,
            since,
            began: since,
            leg,
            by_road: false,
            to,
        }
    }

    /// Clock time it reaches its station.
    fn arrive(&self) -> f32 {
        if self.leg.length() < 0.05 {
            self.since
        } else {
            self.since + self.leg.duration()
        }
    }

    /// True when the chore has been seen for long enough to leave.
    fn has_shown_long_enough(&self, now: f32) -> bool {
        let least = self.chore.least();
        least == 0.0 || now >= self.arrive() + least
    }

    /// True when it is real work, to be rounded off when it ends.
    fn is_real_work(&self) -> bool {
        !matches!(
            self.chore,
            Chore::House | Chore::Stand | Chore::Done | Chore::Vote
        )
    }
}

/// Agents that have just cast a ballot, each with the clock time it
/// did.
#[derive(Resource, Default)]
pub struct Voting(pub HashMap<AgentId, f32>);

/// Chores put on for show: an agent listed here does its chore
/// whatever its work says, while it is working.
#[derive(Resource, Default)]
pub struct Rehearsal(pub HashMap<AgentId, Chore>);

/// The agents whose wait is an objection, which they argue.
#[derive(Resource, Default)]
pub struct Objecting(pub HashSet<AgentId>);

/// The chore a robot at home should be at, and the clock time the
/// step behind it began. `live` is false when only the clock and the
/// society may decide, as for a still.
fn wanted(
    robot: &Robot,
    rehearsal: &Rehearsal,
    seed: u32,
    now: f32,
    live: bool,
) -> (Chore, f32) {
    match robot.state {
        KeeperState::Working => {
            if let Some(&chore) = rehearsal.0.get(&robot.agent) {
                (chore, 0.0)
            } else if live && robot.oops {
                (Chore::Oops, now)
            } else if robot.work.task == Task::Unknown {
                routine(seed, now)
            } else if live && let Some(chore) = robot.queued {
                (chore, robot.step_at)
            } else {
                (Chore::of(robot.work.task), robot.step_at)
            }
        }
        KeeperState::Rest => {
            if live
                && robot.job.as_ref().is_some_and(Job::is_real_work)
            {
                (Chore::Done, now)
            } else {
                (Chore::House, now)
            }
        }
        _ => (Chore::Stand, now),
    }
}

/// The place on its beat a chore starts at.
fn first_beat(chore: Chore, seed: u32) -> f32 {
    chore.bit(0.0, 0.0, seed, 0).beat.map_or(0.0, |b| b.at)
}

/// A job as if the robot had been at it all along, decided by the
/// clock and the society alone.
fn settled(
    robot: &Robot,
    rehearsal: &Rehearsal,
    land: &Land,
    (want, began): (Chore, f32),
    seed: u32,
    motion: &Motion,
) -> Job {
    let lot = &robot.lot;
    let stand = |chore: Chore, beat: f32| {
        // A chore done wherever the robot is needs a place.
        let station = match chore {
            Chore::Oops => Station::Engine,
            Chore::Done => Station::Desk,
            Chore::Vote | Chore::Stand => Station::Porch,
            chore => chore.station(),
        };
        lot.stand(land, station, beat)
            .unwrap_or((lot.porch, lot.yaw))
    };
    let to = stand(want, first_beat(want, seed));
    let by_the_clock = robot.state == KeeperState::Working
        && robot.work.task == Task::Unknown
        && !rehearsal.0.contains_key(&robot.agent);
    let way = if by_the_clock {
        // The round says what came before, so it walks from there.
        let (before, since) = routine(seed, began - 0.01);
        let spent = (began - since - 2.0).max(0.0);
        let beat = before
            .bit(spent, 0.0, seed, 0)
            .beat
            .map_or(0.0, |b| b.at);
        lot.way(land, stand(before, beat).0, to.0)
    } else {
        vec![to.0]
    };
    Job::new(want, began, way, to, motion)
}

/// Has each robot do its chore about its house, or puts it on the
/// road it is walking.
pub(super) fn work_robots(
    time: Res<Time>,
    clock: Res<Clock>,
    land: Res<Land>,
    drawn: Res<Drawn>,
    traffic: Res<Trips>,
    rehearsal: Res<Rehearsal>,
    voting: Res<Voting>,
    objecting: Option<Res<Objecting>>,
    motion: Res<Motion>,
    mut commands: Commands,
    mut robots: Query<
        (Entity, &mut Robot, &mut Keeper, &mut Transform),
        Without<Passage>,
    >,
    mut parts: Query<
        (&mut Transform, &mut Visibility),
        Without<Robot>,
    >,
) {
    let now = clock.t;
    let mut steam = |robot: &mut Robot, ran: Option<f32>| {
        if ran.is_none() && !robot.steaming {
            return;
        }
        robot.steaming = ran.is_some();
        let yard = robot.lot.yard;
        if let Some(ran) = ran
            && let Ok((mut at, _)) = parts.get_mut(robot.engine.wheel)
        {
            *at = yard.wheel(ran * keeper::CRANK_RATE);
        }
        for (i, &ring) in robot.engine.rings.iter().enumerate() {
            let Ok((mut at, mut seen)) = parts.get_mut(ring) else {
                continue;
            };
            let out = ran.and_then(|ran| yard.ring(i, ran));
            let want = match out {
                Some(to) => {
                    *at = to;
                    Visibility::Inherited
                }
                None => Visibility::Hidden,
            };
            if *seen != want {
                *seen = want;
            }
        }
    };

    for (entity, mut robot, mut keeper, mut at) in &mut robots {
        let robot = &mut *robot;
        let ease = (!clock.frozen).then_some(now);
        let trip =
            traffic.trips.iter().find(|t| t.walker == robot.agent);
        if let Some((trip, road)) = trip
            .and_then(|t| drawn.roads.get(&t.pair).map(|r| (t, r)))
        {
            let (p, rotation) = trip.stand(road);
            // In a cart it stands on the bed.
            let p = if trip.rides {
                p + Vec3::Y * land.set.road.cart_bed
            } else {
                p
            };
            if at.translation != p || at.rotation != rotation {
                at.translation = p;
                at.rotation = rotation;
            }
            if keeper.act() != Act::Carry {
                keeper.take_up(Act::Carry, now, ease);
            }
            // On foot it covers the road at a pace it rolls at; in a
            // cart it stands.
            keeper.stride((!trip.rides).then(|| {
                let total = road.route.total();
                Stride {
                    gone: if trip.back {
                        2.0 * total - trip.gone
                    } else {
                        trip.gone
                    },
                    pace: 1.0,
                    speed: land.set.road.walk_speed,
                }
            }));
            robot.job = None;
            robot.playing = None;
            steam(robot, None);
            continue;
        }

        let seed = keeper.who().seed;
        // A still, and a robot's first moment, show the settled
        // state.
        let live = !clock.frozen && robot.placed;
        robot.placed = true;
        if live && robot.state == KeeperState::Working {
            if robot.work.failures > robot.failures {
                robot.oops = true;
            }
            if robot.work.step != robot.step {
                robot.step_at = now;
                // A tool is seen in use even if it is over at once.
                let chore = Chore::of(robot.work.task);
                if chore != Chore::Ponder {
                    robot.queued = Some(chore);
                }
            }
        } else {
            robot.oops = false;
            robot.queued = None;
        }
        robot.step = robot.work.step;
        robot.failures = robot.work.failures;
        if let Some(job) = &robot.job
            && now >= job.arrive()
            && job.has_shown_long_enough(now)
        {
            if robot.queued == Some(job.chore) {
                robot.queued = None;
            }
            if job.chore == Chore::Oops {
                robot.oops = false;
            }
        }

        let (want, began) =
            wanted(robot, &rehearsal, seed, now, live);
        // A ballot is cast whatever else the robot was at.
        let (want, began) = match voting.0.get(&robot.agent) {
            Some(&cast)
                if live && now - cast < Chore::Vote.least() =>
            {
                (Chore::Vote, cast)
            }
            _ => (want, began),
        };
        if !live {
            robot.job = Some(settled(
                robot,
                &rehearsal,
                &land,
                (want, began),
                seed,
                &motion,
            ));
        } else if robot.job.as_ref().is_none_or(|job| {
            want != job.chore
                && (want.is_breaking_in()
                    || job.has_shown_long_enough(now))
        }) {
            let from = at.translation;
            let facing = at.rotation.to_euler(EulerRot::YXZ).0;
            let station = want.station();
            let job = match robot.lot.stand(
                &land,
                station,
                first_beat(want, seed),
            ) {
                Some(to) => {
                    let lot = &robot.lot;
                    // From away it finds a way to its house first.
                    let (way, by_road) = if lot.is_away(&land, from) {
                        let near = lot.approach(&land, to.0);
                        let roads = drawn
                            .roads
                            .values()
                            .map(|road| &road.route)
                            .collect::<Vec<_>>();
                        let Some(back) = houseward(
                            &land,
                            &roads,
                            from.xz(),
                            near.xz(),
                            lot.origin.xz(),
                            |p| footing(&land, p),
                        ) else {
                            // With no dry way it is simply at its
                            // door again.
                            at.translation = lot.porch;
                            at.rotation =
                                Quat::from_rotation_y(lot.yaw);
                            robot.put_down(None);
                            commands.entity(entity).try_insert(
                                Passage::new(Act::Appear),
                            );
                            continue;
                        };
                        let mut way = back.way;
                        way[0] = from;
                        way.pop();
                        way.extend(lot.way(&land, near, to.0));
                        (way, back.by_road)
                    } else {
                        (lot.way(&land, from, to.0), false)
                    };
                    let mut job =
                        Job::new(want, now, way, to, &motion);
                    job.by_road = by_road;
                    if let Some(haste) = robot.haste.take() {
                        job.leg.speed = haste;
                    }
                    job
                }
                None => Job::new(
                    want,
                    now,
                    vec![from],
                    (from, facing),
                    &motion,
                ),
            };
            robot.job = Some(job);
        }
        let Some(job) = robot.job.as_mut() else {
            continue;
        };
        if want == job.chore {
            job.began = began;
        }
        let job = &*job;

        let arrive = job.arrive();
        let lot = &robot.lot;
        let mut ran = None;
        let (p, yaw) = if job.chore == Chore::Stand {
            let objects = robot.state == KeeperState::Waiting
                && objecting
                    .as_ref()
                    .is_some_and(|o| o.0.contains(&robot.agent));
            if objects {
                if keeper.act() != Act::Object {
                    keeper.play(Act::Object, now);
                }
            } else if keeper.act() == Act::Object
                && robot.state == KeeperState::Waiting
            {
                keeper.play(Act::Wait, now);
            } else if keeper.state != robot.state {
                // Its state picks the motion.
                keeper.state = robot.state;
            }
            keeper.stride(Some(Stride::default()));
            robot.playing = None;
            job.to
        } else {
            let (p, yaw, act, since, stride) = if now < arrive {
                let s = job.leg.at(now);
                let act = if job.chore == Chore::House {
                    Act::Walk
                } else {
                    Act::Amble
                };
                // The ground between two points of its way is not
                // a straight line: it keeps to the ground, or to
                // the paving of a yard above it.
                let ground = footing(&land, s.at.xz());
                let paved = ground + lot.yard.shape.terrace.max(0.0);
                // A road it goes home by may be a bridge.
                let top = if job.by_road {
                    paved.max(s.at.y)
                } else {
                    paved
                };
                let p = s.at.with_y(s.at.y.clamp(ground, top));
                (p, s.yaw, act, job.since, s.stride())
            } else {
                let spent = now - job.began.max(arrive);
                let bit = job.chore.bit(
                    now - arrive,
                    spent,
                    seed,
                    robot.step,
                );
                if job.chore == Chore::Run {
                    ran = Some(spent);
                }
                let (p, yaw) = match bit.beat {
                    Some(b) => {
                        let (p, out) = lot
                            .stand(&land, Station::Beat, b.at)
                            .unwrap_or(job.to);
                        let along = lot.yaw + lot.yard.along();
                        let yaw = if b.heading > 0.0 {
                            along
                        } else if b.heading < 0.0 {
                            along + std::f32::consts::PI
                        } else {
                            out
                        };
                        (p, yaw)
                    }
                    None => job.to,
                };
                let stride = bit
                    .beat
                    .map_or(Stride::default(), |b| b.stride());
                (p, yaw, bit.act, arrive + bit.since, stride)
            };
            keeper.stride(Some(stride));
            if robot.playing.is_none_or(|(a, s)| {
                a != act || (s - since).abs() > 0.02
            }) {
                keeper.take_up(act, since, ease.filter(|_| live));
                robot.playing = Some((act, since));
            }
            (p, yaw)
        };

        let to = Quat::from_rotation_y(yaw);
        // A turn takes a moment.
        let rotation = if live {
            at.rotation.slerp(
                to,
                1.0 - (-time.delta_secs() * motion.turn_rate).exp(),
            )
        } else {
            to
        };
        if at.translation != p || at.rotation != rotation {
            at.translation = p;
            at.rotation = rotation;
        }
        steam(robot, ran);
    }
}
