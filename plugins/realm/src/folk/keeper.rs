//! The robots (docs/robots.md): a figure in brass and wood for each
//! agent, whose two lanterns show its state, at work about its house
//! (docs/working-alone.md).

mod acts;
mod homing;
mod kit;
mod lot;
mod pose;
mod rig;
mod robot;
mod trip;
mod who;
mod work;

use std::collections::{HashMap, HashSet};

use glam::{Quat, Vec3};
use hamlet_engine_bevy::Reply;
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, NeedKind,
};
use hamlet_engine_bevy::prelude::*;

use crate::bridge::{Bridge, Moment};
use crate::build::frame::Frame;
use crate::build::upload::Draw;
pub use crate::folk::keeper::acts::Act;
use crate::folk::keeper::kit::Kit;
pub use crate::folk::keeper::lot::{Floor, Lot};
use crate::folk::keeper::rig::Rig;
pub use crate::folk::keeper::robot::{Passage, Robot};
use crate::folk::keeper::who::Who;
use crate::folk::sounds::{FolkCue, FolkSound};
use crate::places::house::yard_of;
use crate::places::road::Roads;
use crate::realm::{Ctx, Shared};
use crate::society::{Agent, AgentId};
use crate::view::camera::{Camera, View};
use crate::view::pick::Thing;
use crate::world::layout::{HALL_PLOT, Route, plot};
use crate::world::palette::Palette;

/// The most robots made in one frame.
const MADE_PER_FRAME: usize = 6;

/// The most robots whose lanterns throw light at once, of the lights
/// the engine lets a plugin hold.
const LANTERNS: usize = 12;

/// The robots drawn.
#[derive(Resource)]
pub struct Robots {
    kit: Kit,
    robots: HashMap<AgentId, Robot>,
    /// Robots on their way out, kept until their passage is over.
    leaving: Vec<Robot>,
    /// Passages asked for that are yet to begin, by agent.
    asked: HashMap<AgentId, Act>,
    /// True once the robots of the first society seen are made:
    /// those that come later are seen arriving.
    met: bool,
    /// The agents whose wait is an objection, which they argue.
    objecting: HashSet<AgentId>,
    /// The sound each robot posed last frame made, and its count.
    heard: HashMap<AgentId, (FolkSound, f32)>,
}

impl Robots {
    /// No robots yet, their kit to be built as they update.
    pub fn make(
        pal: &Palette,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        Ok(Self {
            kit: Kit::make(pal, draw)?,
            robots: HashMap::new(),
            leaving: Vec::new(),
            asked: HashMap::new(),
            met: false,
            objecting: HashSet::new(),
            heard: HashMap::new(),
        })
    }

    /// Brings the robots in line with the society and moves each a
    /// frame `dt` seconds further. The sounds the robots make are
    /// told to `cue`. A robot away from home comes back over the
    /// land or by `roads`.
    pub fn update(
        &mut self,
        ctx: &Ctx,
        sight: &Sight,
        roads: &[&Route],
        dt: f32,
        cue: &mut impl FnMut(FolkCue),
        draw: &mut Draw,
    ) {
        if !self.kit.hand_over(ctx.pal, draw) {
            return;
        }
        let now = ctx.clock;
        self.sync(ctx, now, draw);
        let (land, motion) = (ctx.land, &ctx.tuning.motion);
        let detail = &ctx.tuning.detail;
        let shown = |robot: &Robot| {
            robot.rig.at.at.distance(ctx.eye) < detail.robots_far
                && !robot.folded(now)
        };
        // A pose is a function of the clock alone, so a robot out of
        // the view is left as it was and picks up when it is seen
        // again.
        let posed = |robot: &Robot| {
            shown(robot)
                && sight.sees(robot.rig.at.at, detail.robot_sight)
        };
        let mut heard = HashMap::new();
        for robot in self.robots.values_mut() {
            robot.pass(now);
            let objects = self.objecting.contains(&robot.agent);
            robot.work(land, roads, motion, objects, now, dt);
            robot.rig.show(shown(robot), draw);
            let seen = posed(robot);
            robot
                .rig
                .pose(now, motion, &self.kit, ctx.stock, seen, draw);
            if seen {
                robot.listen(now, &self.heard, &mut heard, cue);
            }
        }
        // After dark the lanterns nearest the camera throw light.
        let look = &ctx.sky.look;
        let mut lit = self
            .robots
            .values()
            .filter(|robot| look.candle > 0.0 && posed(robot))
            .map(|robot| {
                (
                    robot.agent,
                    robot.rig.at.at.distance_squared(ctx.eye),
                )
            })
            .collect::<Vec<_>>();
        lit.sort_by(|a, b| a.1.total_cmp(&b.1));
        lit.truncate(LANTERNS);
        for robot in self.robots.values_mut() {
            let on =
                lit.iter().any(|(agent, _)| *agent == robot.agent);
            robot
                .rig
                .lantern(on.then_some(look.lantern_lumens), draw);
        }
        self.leaving.retain_mut(|robot| {
            let gone = robot.pass(now) || robot.rig.gone(now);
            robot.rig.show(shown(robot), draw);
            let seen = posed(robot);
            robot
                .rig
                .pose(now, motion, &self.kit, ctx.stock, seen, draw);
            if seen {
                robot.listen(now, &self.heard, &mut heard, cue);
            }
            if gone {
                robot.rig.despawn(draw);
            }
            !gone
        });
        self.heard = heard;
    }

    /// Makes the robot of each living agent that has none, or whose
    /// house changed, and takes away those of agents that are gone.
    fn sync(&mut self, ctx: &Ctx, now: f32, draw: &mut Draw) {
        let society = ctx.society;
        let mut made = 0;
        for agent in society.agents.iter().filter(|a| !a.archived) {
            let Some(town) = society.town(agent.town) else {
                continue;
            };
            let leader = town.leader == agent.id;
            let at = ctx.land.sites[town.site].at;
            let sig =
                (town.site, at, agent.plot, agent.wings, leader);
            if let Some(robot) = self.robots.get_mut(&agent.id)
                && robot.sig == sig
            {
                robot.follow(
                    agent.state,
                    agent.work,
                    now,
                    ctx.tuning.play.cheer_window,
                );
                continue;
            }
            if made >= MADE_PER_FRAME {
                continue;
            }
            made += 1;
            let was = self.robots.remove(&agent.id);
            // A robot whose house stands up anew comes once it
            // stands.
            let rises = self.met
                && was.as_ref().is_none_or(|was| {
                    (was.sig.0, was.sig.1, was.sig.2)
                        != (sig.0, sig.1, sig.2)
                });
            if let Some(was) = was {
                was.rig.despawn(draw);
            }
            match self.raise(ctx, agent, town.site, sig, draw) {
                Ok(mut robot) => {
                    if rises {
                        let after =
                            ctx.tuning.house_motion.props_after;
                        robot.passage = Some(Passage::new(
                            Act::Appear,
                            now + after,
                        ));
                    }
                    self.robots.insert(agent.id, robot);
                }
                Err(why) => {
                    hamlet_engine_bevy::warn(&format!(
                        "a robot: {why}"
                    ));
                }
            }
        }
        if made == 0 {
            self.met = true;
        }
        // Robots of agents that left go when their bow is over.
        let gone = self
            .robots
            .keys()
            .filter(|agent| {
                society.agent(**agent).is_none_or(|a| a.archived)
            })
            .copied()
            .collect::<Vec<_>>();
        for agent in gone {
            let Some(mut robot) = self.robots.remove(&agent) else {
                continue;
            };
            match self.asked.remove(&agent) {
                Some(act) if act.vanishes() => {
                    robot.passage = Some(Passage::new(act, now));
                    self.leaving.push(robot);
                }
                _ => robot.rig.despawn(draw),
            }
        }
        for (agent, act) in self.asked.drain() {
            if let Some(robot) = self.robots.get_mut(&agent) {
                robot.passage = Some(Passage::new(act, now));
            }
        }
    }

    /// The robot of `agent`, standing at its door.
    fn raise(
        &self,
        ctx: &Ctx,
        agent: &Agent,
        site: usize,
        sig: (usize, glam::Vec2, u32, u8, bool),
        draw: &mut Draw,
    ) -> Result<Robot, String> {
        let land = ctx.land;
        let spot = plot(land, site, agent.plot);
        let yard = yard_of(
            land,
            agent.id,
            agent.plot == HALL_PLOT,
            spot.yaw,
            agent.wings,
        );
        let lot = Lot::new(land, &spot, yard);
        let who = if sig.4 {
            Who::leader()
        } else {
            Who::citizen(agent.id.0 * 7 + 3)
        };
        let at = Frame::at(lot.porch)
            .with_turn(Quat::from_rotation_y(lot.yaw));
        let rig = Rig::make(
            &self.kit,
            ctx.stock,
            who,
            at,
            agent.state,
            Thing::Robot(agent.id).key(),
            draw,
        )?;
        Ok(Robot::new(
            agent.id,
            rig,
            lot,
            agent.state,
            agent.work,
            sig,
        ))
    }

    /// Has each robot seen doing what befell its agent.
    pub fn moments(&mut self, moments: &[Moment]) {
        for moment in moments {
            match *moment {
                Moment::Left {
                    agent,
                    into: Some(host),
                } => {
                    self.asked.insert(agent, Act::MergeInto);
                    self.asked.insert(host, Act::MergeHost);
                }
                Moment::Left { agent, into: None } => {
                    self.asked.insert(agent, Act::Leave);
                }
                // One that comes back is seen arriving as its house
                // stands again.
                Moment::Back(_) => {}
            }
        }
    }

    /// Where the robot of `agent` stands.
    pub fn at(&self, agent: AgentId) -> Option<Vec3> {
        self.robots.get(&agent).map(|robot| robot.rig.at.at)
    }

    /// The agent of the robot whose box of `size`, standing on the
    /// ground with it and turned as it is, the ray from `from` along
    /// `way` enters first. A box the ray begins in is passed over,
    /// as is a robot in the hand, so that the pointer reaches what is
    /// under it.
    pub fn under(
        &self,
        from: Vec3,
        way: Vec3,
        size: Vec3,
    ) -> Option<AgentId> {
        let half = size * 0.5;
        self.robots
            .iter()
            .filter(|(_, robot)| !robot.handled())
            .filter_map(|(agent, robot)| {
                let at = robot.rig.at;
                let back = at.turn.inverse();
                let from = back * (from - at.at) - Vec3::Y * half.y;
                let way = (back * way).recip();
                let (a, b) =
                    ((-half - from) * way, (half - from) * way);
                let (near, far) =
                    (a.min(b).max_element(), a.max(b).min_element());
                (near.is_finite() && near > 0.0 && near <= far)
                    .then_some((near, *agent))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, agent)| agent)
    }

    /// The ground of each robot's yard, for a [`Floor`].
    pub fn lots(&self) -> Vec<Lot> {
        self.robots.values().map(|robot| robot.lot).collect()
    }

    /// The robot of `agent`.
    pub fn robot(&self, agent: AgentId) -> Option<&Robot> {
        self.robots.get(&agent)
    }

    pub fn robot_mut(
        &mut self,
        agent: AgentId,
    ) -> Option<&mut Robot> {
        self.robots.get_mut(&agent)
    }

    /// Every robot in the realm, those on their way out left out.
    pub fn each_mut(&mut self) -> impl Iterator<Item = &mut Robot> {
        self.robots.values_mut()
    }
}

/// A robot's walk.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    /// Ground covered per second at a walk.
    pub walk_speed: f32,
    /// Ground covered by two steps of a robot of size 1.
    pub stride: f32,
    /// Seconds a walk takes to reach its speed, and to stop.
    pub get_going: f32,
    /// Distance either side of a corner over which the turn is
    /// spread.
    pub corner: f32,
    /// Ground a robot covers per second about its own house.
    pub amble: f32,
    /// Rate at which a robot at its chores turns to face a new way.
    pub turn_rate: f32,
    /// Ground covered per second above which a robot rolls on its
    /// wheels.
    pub roll_above: f32,
    /// Seconds a robot takes to change between stepping and rolling.
    pub roll_ease: f32,
}

/// What a robot shows of its agent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KState {
    Rest,
    Working,
    Waiting,
    Fault,
    Failed,
}

/// Brings the robots in line with the society and moves each a
/// frame further.
pub fn move_robots(
    mut robots: ResMut<Robots>,
    shared: Shared,
    camera: Res<Camera>,
    shape: Res<ViewShape>,
    roads: Res<Roads>,
    mut cues: MessageWriter<FolkCue>,
    mut draw: Draw,
) {
    let floor = shared.floor();
    let ctx = shared.ctx(&floor);
    let sight = Sight::of(
        ctx.eye,
        camera.views().0,
        ctx.tuning.camera.fov,
        shape.0,
    );
    robots.update(
        &ctx,
        &sight,
        &roads.routes(),
        shared.clock.dt,
        &mut |cue| {
            cues.write(cue);
        },
        &mut draw,
    );
}

/// The shape of the realm's view: its width over its height.
#[derive(Resource)]
pub struct ViewShape(pub f32);

impl Default for ViewShape {
    fn default() -> Self {
        Self(16.0 / 9.0)
    }
}

/// Follows the size of the realm's view.
pub fn follow_view(
    mut inputs: MessageReader<SceneInput>,
    camera: Res<Camera>,
    mut shape: ResMut<ViewShape>,
) {
    for input in inputs.read() {
        if let SceneInput::Resize(resize) = input
            && resize.view == camera.view()
            && resize.height > 0.0
        {
            shape.0 = resize.width / resize.height;
        }
    }
}

/// What the camera sees, as the cone about its view.
pub struct Sight {
    eye: Vec3,
    ahead: Vec3,
    /// The cone's half angle, out to the view's corners.
    half: f32,
}

impl Sight {
    /// The sight from `eye` along `view`, with a vertical field of
    /// `fov` degrees and `aspect` its width over its height.
    pub fn of(eye: Vec3, view: View, fov: f32, aspect: f32) -> Self {
        let pitch = view.pitch.to_radians();
        let back = Vec3::new(
            view.yaw.sin() * pitch.cos(),
            pitch.sin(),
            view.yaw.cos() * pitch.cos(),
        );
        let tall = (fov.to_radians() * 0.5).tan();
        Self {
            eye,
            ahead: -back,
            half: (tall * (1.0 + aspect * aspect).sqrt()).atan(),
        }
    }

    /// True when any of the ball of `radius` about `at` is in sight.
    pub fn sees(&self, at: Vec3, radius: f32) -> bool {
        let to = at - self.eye;
        let along = to.dot(self.ahead);
        let off = (to - self.ahead * along).length();
        off * self.half.cos() - along * self.half.sin() <= radius
    }
}

/// Follows what waits for the chief: an agent whose wait is an
/// objection argues it.
pub fn follow_needs(
    mut replies: MessageReader<Replied>,
    mut events: MessageReader<AgentsEventReceived>,
    bridge: Res<Bridge>,
    mut robots: ResMut<Robots>,
) {
    let replied = replies.read().filter_map(|replied| match &replied
        .reply
    {
        Ok(Reply::Agents(AgentsReply::Needs(needs))) => Some(needs),
        _ => None,
    });
    let told = events.read().filter_map(|event| match &event.event {
        AgentsEvent::Needs(needs) => Some(needs),
        _ => None,
    });
    let Some(needs) = replied.chain(told).last() else {
        return;
    };
    robots.objecting = needs
        .iter()
        .filter(|need| need.kind == NeedKind::Objection)
        .filter_map(|need| bridge.find(&need.by))
        .collect();
}
