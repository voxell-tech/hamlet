//! Wickstep: the mascot goes out like a candle where it stands and is
//! lit again somewhere else. Over a short way it walks.

mod charm;

use std::f32::consts::{PI, TAU};

use glam::{Vec2, Vec3, Vec3Swizzles};

use self::charm::chance;
pub use self::charm::{Charm, shapes};
use super::ChiefSettings;
use super::homing::Walking;
use super::mascot::{Doing, Pose};
use super::place::{self, Pastime};
use crate::frame::Frame;
use crate::land::Land;
use crate::mesh::smoothstep;
use crate::society::AgentId;

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
    With(AgentId),
}

/// One end of a Wickstep, for its circle and motes to be cast: where
/// on the ground, and whether the mascot arrives there.
#[derive(Clone, Copy, Debug)]
pub struct End {
    pub ground: Vec3,
    pub arrives: bool,
}

/// What a mascot is told each frame: where it lives, who the chief
/// talks to, and where the camera is.
pub struct Whereabouts<'a> {
    pub land: &'a Land,
    /// The frame of its place.
    pub home: &'a Frame,
    /// The agent the chief talks to, and where its robot stands.
    pub beside: Option<(AgentId, Vec3)>,
    pub eye: Vec3,
    /// The fastest it hurries after an agent that gets ahead.
    pub fastest: f32,
}

/// A chief's mascot.
#[derive(Debug)]
pub struct Mascot {
    /// Its place, or the place it is leaving.
    pub stay: Stay,
    pub phase: Phase,
    pub doing: Doing,
    /// The pose it was last drawn in, the pose it eases from and
    /// the clock time that began.
    pub shown: Pose,
    from: Pose,
    changed: f32,
    /// Where it stands, and the way it faces, as a rotation about Y
    /// from +Z.
    pub at: Vec3,
    pub yaw: f32,
    /// Its offset from its agent.
    side: Vec2,
    /// The way it walks after its agent, as far as where it was
    /// going when the way was found.
    way: Vec<Vec2>,
    /// The spot of its grounds it is at or walking to, and the
    /// clock time it moves on.
    spot: usize,
    until: f32,
    /// Counts its Wicksteps and its choices of a spot.
    turn: u32,
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

impl Mascot {
    /// A mascot standing at `at`, facing `yaw`, that stays where it
    /// is until the clock time `until`.
    pub fn new(at: Vec3, yaw: f32, now: f32, until: f32) -> Self {
        let pose = Pose::of(Doing::Idle, now);
        Self {
            stay: Stay::Home,
            phase: Phase::Lit,
            doing: Doing::Idle,
            shown: pose,
            from: pose,
            changed: f32::MIN,
            at,
            yaw,
            side: Vec2::ZERO,
            way: Vec::new(),
            spot: 0,
            until,
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

    /// The pose it takes at `now`, eased from the one before and bent
    /// by a Wickstep.
    pub fn pose(&mut self, set: &ChiefSettings, now: f32) -> Pose {
        let to = Pose::of(self.doing, now);
        let k = smoothstep(0.0, set.blend, now - self.changed);
        let pose = self.from.toward(&to, k);
        self.shown = pose;
        pose.stepping(self.phase, now)
    }

    /// Moves it a frame `dt` seconds on, at clock time `now`: a
    /// Wickstep to whoever its chief talks to and home again, and on
    /// foot between. The ends of a Wickstep that began.
    pub fn step(
        &mut self,
        set: &ChiefSettings,
        about: &Whereabouts,
        now: f32,
        dt: f32,
    ) -> Vec<End> {
        let mut ends = Vec::new();
        let land = about.land;
        let here = self.at;
        // Toward the camera and to its right, along the ground.
        let back = (about.eye - here).xz().normalize_or(Vec2::Y);
        let right = Vec2::new(back.y, -back.x);
        let spots = place::spots(set);
        let spot_at = |i: usize| {
            about
                .home
                .point(Vec3::new(spots[i].at.x, 0.0, spots[i].at.y))
                .xz()
        };
        // An agent that is nowhere to be found is not stood by.
        let of = |agent: AgentId| {
            about
                .beside
                .filter(|(them, _)| *them == agent)
                .map(|(_, at)| at)
        };
        let wanted = match about.beside {
            Some((agent, _)) => Stay::With(agent),
            None => Stay::Home,
        };
        let side = right * set.beside + back * set.before;

        match self.phase {
            Phase::Lit if wanted != self.stay => {
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
                        let stand = land
                            .beside(them, side, set.restep)
                            .unwrap_or(them + side);
                        self.side = stand - them;
                        self.stay = wanted;
                        self.way.clear();
                    }
                    None => {
                        self.phase = Phase::Out {
                            began: now,
                            long: set.out_time,
                        };
                    }
                }
            }
            Phase::Lit => {
                let (target, rest, look_at) = match self.stay {
                    Stay::With(agent) => {
                        let them = of(agent).unwrap_or(here).xz();
                        // Its agent has gone where its own spot is
                        // wet or taken.
                        if !land.clear(them + self.side) {
                            let side = self.side.normalize_or(right)
                                * Vec2::new(set.beside, set.before)
                                    .length();
                            if let Some(stand) =
                                land.beside(them, side, set.restep)
                            {
                                self.side = stand - them;
                            }
                        }
                        (
                            them + self.side,
                            Doing::Attend,
                            them + back * set.cheat,
                        )
                    }
                    Stay::Home => {
                        if now >= self.until {
                            // On to another spot. It lingers there
                            // from when it arrives.
                            self.turn = self.turn.wrapping_add(1);
                            let roll = chance(self.turn as f32 * 1.7);
                            let others = spots.len() - 1;
                            let hop = 1
                                + (roll * others as f32) as usize
                                    % others;
                            self.spot =
                                (self.spot + hop) % spots.len();
                            self.until = f32::MAX;
                        }
                        let spot = spots[self.spot];
                        let rest = match spot.pastime {
                            Pastime::Wait => Doing::Idle,
                            Pastime::Read => Doing::Read,
                            Pastime::Tend => Doing::Tend,
                            Pastime::Gaze => Doing::Gaze,
                        };
                        let faces = about
                            .home
                            .point(Vec3::new(
                                spot.faces.x,
                                0.0,
                                spot.faces.y,
                            ))
                            .xz();
                        (spot_at(self.spot), rest, faces)
                    }
                };
                let gap = target - here.xz();
                let far = gap.length();
                let walking = self.doing == Doing::Walk;
                // Beside an agent it lets it stray a little before it
                // follows.
                let slack = match self.stay {
                    Stay::With(_) if !walking => set.slack,
                    _ => 0.05,
                };
                let mut way = look_at - here.xz();
                if matches!(self.stay, Stay::With(_))
                    && far > set.afoot
                {
                    // Too far to walk: it steps after its agent.
                    self.phase = Phase::Out {
                        began: now,
                        long: set.out_time,
                    };
                } else if far > slack {
                    let next = match self.stay {
                        Stay::With(_) => {
                            let found =
                                self.way.last().is_some_and(|end| {
                                    end.distance(target) < REPLAN
                                });
                            // True when there is a dry way on foot:
                            // the
                            // one it is on, or one found now.
                            let mut dry = found;
                            if !found {
                                let way =
                                    land.overland(here.xz(), target);
                                dry = way.is_some();
                                self.way = way.unwrap_or_default();
                            }
                            let passed = self
                                .way
                                .iter()
                                .take_while(|p| {
                                    p.distance(here.xz()) < 0.05
                                })
                                .count();
                            self.way.drain(..passed);
                            leg(&self.way, target, dry)
                        }
                        Stay::Home => Some(target),
                    };
                    if let Some(next) = next {
                        // The further its agent is ahead, the faster
                        // it goes, up to as
                        // fast as a robot does.
                        let behind = match self.stay {
                            Stay::With(_) => ((far - set.slack)
                                / (set.restep * 0.5 - set.slack))
                                .clamp(0.0, 1.0),
                            Stay::Home => 0.0,
                        };
                        let top = about.fastest.max(set.walk_speed);
                        let speed = set.walk_speed
                            + (top - set.walk_speed) * behind;
                        let leg = next - here.xz();
                        let stride = (speed * dt).min(leg.length());
                        self.at = land.on_ground(
                            here.xz()
                                + leg.normalize_or_zero() * stride,
                        );
                        way = leg;
                        self.act(Doing::Walk, now);
                    } else {
                        // No dry way on foot: it steps after its
                        // agent.
                        self.phase = Phase::Out {
                            began: now,
                            long: set.out_time,
                        };
                    }
                } else {
                    self.way.clear();
                    if self.until == f32::MAX {
                        let (least, most) = set.linger;
                        let roll = chance(self.turn as f32 + 0.37);
                        self.until =
                            now + least + (most - least) * roll;
                    }
                    self.act(rest, now);
                }
                if way.length_squared() > 1e-6 {
                    self.yaw = turned(
                        self.yaw,
                        facing(way),
                        set.turn_rate * dt,
                    );
                }
            }
            Phase::Out { began, long } if now - began >= long => {
                self.phase = Phase::Dark {
                    began: now,
                    long: set.dark_time,
                };
            }
            Phase::Dark { began, long } if now - began >= long => {
                // Sets it down where `wanted` has it.
                self.stay = wanted;
                self.way.clear();
                self.yaw = match wanted {
                    Stay::With(agent) => {
                        let them = of(agent).unwrap_or(here).xz();
                        let stand = land
                            .beside(them, side, set.restep)
                            .unwrap_or(them + side);
                        self.side = stand - them;
                        self.at = land.on_ground(stand);
                        facing(back * set.cheat - self.side)
                    }
                    Stay::Home => {
                        self.spot = 0;
                        self.until = now + set.linger.0 * 0.5;
                        self.at = land.on_ground(spot_at(0));
                        facing((about.home.turn * Vec3::Z).xz())
                    }
                };
                self.phase = Phase::In {
                    began: now,
                    long: set.in_time,
                };
                self.doing = match wanted {
                    Stay::With(_) => Doing::Attend,
                    Stay::Home => Doing::Idle,
                };
                self.changed = f32::MIN;
                self.turn = self.turn.wrapping_add(1);
                ends.push(End {
                    ground: self.at,
                    arrives: true,
                });
            }
            Phase::In { began, long } if now - began >= long => {
                self.phase = Phase::Lit;
            }
            Phase::Out { .. }
            | Phase::Dark { .. }
            | Phase::In { .. } => {}
        }
        // It has just begun to go out.
        if let Phase::Out { began, .. } = self.phase
            && began == now
        {
            ends.push(End {
                ground: here,
                arrives: false,
            });
        }
        ends
    }
}
