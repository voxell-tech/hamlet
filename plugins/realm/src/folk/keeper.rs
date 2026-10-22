//! The robots (docs/robots.md): a figure in brass and wood for each
//! agent, whose two lanterns show its state, at work about its house
//! (docs/working-alone.md).

mod acts;
pub(crate) mod kit;
mod lot;
pub(crate) mod pose;
pub(crate) mod rig;
mod robot;
mod trip;
pub mod wear;
pub(crate) mod who;
mod work;

use std::collections::HashMap;

use glam::{Quat, Vec3};

use crate::bridge::Moment;
use crate::build::frame::Frame;
use crate::build::upload::Upload;
use crate::folk::keeper::acts::Act;
use crate::folk::keeper::kit::Kit;
pub use crate::folk::keeper::kit::statue::Statue;
use crate::folk::keeper::lot::Lot;
pub use crate::folk::keeper::lot::{Floor, Pad};
use crate::folk::keeper::rig::Rig;
use crate::folk::keeper::robot::{Passage, Robot};
use crate::folk::keeper::wear::Wear;
use crate::folk::keeper::who::Who;
use crate::places::house::yard_of;
use crate::realm::Ctx;
use crate::society::{Agent, AgentId};
use crate::view::pick::Thing;
use crate::wardrobe::AgentLook;
use crate::world::land::Land;
use crate::world::layout::{HALL_PLOT, plot};
use crate::world::palette::Palette;

/// The most robots made in one frame.
const MADE_PER_FRAME: usize = 6;

/// The robots drawn.
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
    /// What the robots wear, and the version of the wardrobe it
    /// follows.
    wear: Wear,
    dressed: u64,
}

impl Robots {
    /// No robots yet, their kit to be built as they update.
    pub fn make(pal: &Palette) -> Result<Self, String> {
        Ok(Self {
            kit: Kit::make(pal)?,
            robots: HashMap::new(),
            leaving: Vec::new(),
            asked: HashMap::new(),
            met: false,
            wear: Wear::default(),
            dressed: 0,
        })
    }

    /// Brings the robots in line with the society and moves each a
    /// frame `dt` seconds further.
    pub fn update(
        &mut self,
        ctx: &Ctx,
        dt: f32,
        upload: &mut Upload,
    ) {
        if !self.kit.hand_over(ctx.pal, upload) {
            return;
        }
        let now = ctx.clock;
        let wardrobe = ctx.wardrobe;
        let mut redress = self.dressed != wardrobe.version;
        if redress {
            self.wear.follow(wardrobe, ctx.pal);
            self.dressed = wardrobe.version;
        }
        redress |= self.wear.hand_over(upload);
        self.sync(ctx, now);
        if redress {
            for (agent, robot) in &mut self.robots {
                let look = wardrobe
                    .of_agent(*agent)
                    .cloned()
                    .unwrap_or_default();
                if look != robot.look || !robot.fitted {
                    Self::dress_in(
                        &self.wear,
                        (&self.kit, ctx),
                        robot,
                        look,
                    );
                }
            }
        }
        let (land, motion) = (ctx.land, &ctx.tuning.motion);
        let moving = (motion, &ctx.tuning.wear);
        let far = ctx.tuning.detail.robots_far;
        let seen = |robot: &Robot| {
            robot.rig.at.at.distance(ctx.eye) < far
                && !robot.folded(now)
        };
        for robot in self.robots.values_mut() {
            robot.pass(now);
            robot.work(land, motion, now, dt);
            let shown = seen(robot);
            robot.rig.show(shown);
            robot.rig.pose(now, moving, &self.kit, ctx.stock, shown);
        }
        self.leaving.retain_mut(|robot| {
            let gone = robot.pass(now) || robot.rig.gone(now);
            let shown = seen(robot);
            robot.rig.show(shown);
            robot.rig.pose(now, moving, &self.kit, ctx.stock, shown);
            !gone
        });
    }

    /// Makes the robot of each living agent that has none, or whose
    /// house changed, and takes away those of agents that are gone.
    fn sync(&mut self, ctx: &Ctx, now: f32) {
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
                robot.follow(agent.state, agent.work, now);
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
            match self.raise(ctx, agent, town.site, sig) {
                Ok(mut robot) => {
                    if let Some(look) =
                        ctx.wardrobe.of_agent(agent.id)
                    {
                        Self::dress_in(
                            &self.wear,
                            (&self.kit, ctx),
                            &mut robot,
                            look.clone(),
                        );
                    }
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
                    hamlet_engine_sdk::warn(&format!(
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
            if let Some(act) = self.asked.remove(&agent)
                && act.vanishes()
            {
                robot.passage = Some(Passage::new(act, now));
                self.leaving.push(robot);
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
        let who = Who::of(agent.id, sig.4);
        let at = Frame::at(lot.porch)
            .with_turn(Quat::from_rotation_y(lot.yaw));
        let rig = Rig::make(
            &self.kit,
            ctx.stock,
            who,
            at,
            agent.state,
            Thing::Robot(agent.id).key(),
        )?;
        Ok(Robot::new(rig, lot, agent.state, agent.work, sig))
    }

    /// Dresses `robot` in `look`.
    fn dress_in(
        wear: &Wear,
        (kit, ctx): (&Kit, &Ctx),
        robot: &mut Robot,
        look: AgentLook,
    ) {
        robot.fitted = !wear.waits(&look);
        robot.rig.dress(
            kit,
            ctx.stock,
            wear.dress(&look),
            look.clip.as_ref(),
        );
        robot.look = look;
    }

    /// Has the robot of `agent` play the clip it wears, at clock time
    /// `now`.
    pub fn gesture(&mut self, agent: AgentId, now: f32) {
        if let Some(robot) = self.robots.get_mut(&agent) {
            robot.rig.gesture(now);
        }
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

    /// The floor of `land` and of the robots' yards.
    pub fn floor<'a>(&self, land: &'a Land) -> Floor<'a> {
        Floor::new(
            land,
            self.robots.values().map(|robot| robot.lot).collect(),
        )
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
