//! The robots (docs/robots.md): a figure in brass and wood for each
//! agent, whose two lanterns show its state.

mod acts;
mod animate;
mod kit;
mod pose;
mod spawn;
#[cfg(test)]
mod tests;
mod trip;
pub mod wear;
mod who;
pub mod work;

use bevy::prelude::*;

pub use self::acts::{
    Act, CRANK_RATE, MERGE_GAP, SEAT_BACK, TALK_GAP,
};
use self::acts::{Context, Quirk};
pub(crate) use self::animate::animate;
pub use self::kit::Kit;
pub use self::kit::parts::{MESH_PARTS, default_parts};
use self::pose::{Pose, Prop, arc, ramp, rx, ry};
pub use self::spawn::{spawn_agent, spawn_as, spawn_dressed};
pub use self::trip::{Step, Stride, Trip};
pub use self::wear::{Gesture, Wearer, Wears};
pub use self::who::{Emblem, Hat, Paint, Who};
use crate::sound::Sound;

/// The robots' walk, and their poses each frame.
pub fn plugin(app: &mut App) {
    app.add_plugins(wear::plugin)
        .init_resource::<Motion>()
        .add_systems(Update, animate.in_set(crate::Step::Robots));
}

/// A robot's walk.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
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
    /// Seconds a worn clip takes to ease in, and out.
    pub emote_blend: f32,
    /// The most, as a share of its `every`, an idle robot waits
    /// beyond `every` before it plays its clip again.
    pub emote_spread: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeeperState {
    Rest,
    Working,
    Waiting,
    Fault,
    Failed,
}

impl KeeperState {
    pub const ALL: [Self; 5] = [
        Self::Rest,
        Self::Working,
        Self::Waiting,
        Self::Fault,
        Self::Failed,
    ];

    pub fn next(self) -> Self {
        match self {
            Self::Rest => Self::Working,
            Self::Working => Self::Waiting,
            Self::Waiting => Self::Fault,
            Self::Fault => Self::Failed,
            Self::Failed => Self::Rest,
        }
    }

    /// True when this state moves by itself, so frames must keep
    /// coming.
    pub fn is_animated(self) -> bool {
        matches!(self, Self::Working | Self::Waiting | Self::Fault)
    }
}

/// One robot. Set `state` for the plain motion of a state, or call
/// `play` for a particular one.
#[derive(Component)]
pub struct Keeper {
    pub state: KeeperState,
    who: Who,
    quirk: Quirk,
    act: Act,
    /// Clock time the motion began.
    since: f32,
    /// The pose it was in when the motion began, and the motion it
    /// left.
    from: Option<(Pose, Act)>,
    /// The state the motion was last chosen for.
    synced: KeeperState,
    gait: f32,
    pace: f32,
    /// Its change from stepping, 0, to rolling, 1.
    roll: f32,
    /// Clock time it was last drawn.
    seen: f32,
    shown: Prop,
    lit_as: Vec3,
    props: [Handle<Mesh>; 9],
    glints: [Option<Handle<Mesh>>; 9],
    /// The walk someone else moves it along.
    strode: Option<Stride>,
    /// Turn and lift of its head toward something it watches, on top
    /// of its motion.
    look: Vec2,
    parts: Parts,
}

struct Parts {
    rig: Entity,
    body: Entity,
    head: Entity,
    hat: Entity,
    eye_l: Entity,
    eye_r: Entity,
    arm_l: Entity,
    arm_r: Entity,
    leg_l: Entity,
    leg_r: Entity,
    wheel_l: Entity,
    wheel_r: Entity,
    pole: Entity,
    lamp: Entity,
    ring: Entity,
    light: Option<Entity>,
    prop: Entity,
    glint: Entity,
    key: Entity,
    flag: Option<Entity>,
    lit: Handle<StandardMaterial>,
    /// Every mesh of its parts, in the order they were spawned.
    shapes: Vec<wear::Shape>,
}

/// A clock time long before any still, for a motion that has already
/// played out.
const LONG_AGO: f32 = -1000.0;

impl Keeper {
    pub fn act(&self) -> Act {
        self.act
    }

    pub fn who(&self) -> Who {
        self.who
    }

    /// Clock time its motion began.
    pub fn since(&self) -> f32 {
        self.since
    }

    /// Begins a motion at clock time `now`, easing out of the pose it
    /// is in then.
    pub fn play(&mut self, act: Act, now: f32) {
        self.start(act, now);
        self.state = act.state();
        self.synced = self.state;
    }

    /// Takes up a motion that began at clock time `since`. With
    /// `now` it eases out of the pose it is in then; without, it is
    /// as if it had been in the motion all along.
    pub fn take_up(
        &mut self,
        act: Act,
        since: f32,
        now: Option<f32>,
    ) {
        self.from = now.map(|now| (self.eval(now), self.act));
        self.act = act;
        self.since = since;
        self.state = act.state();
        self.synced = self.state;
    }

    /// Sets the stride of a robot that is being moved over the
    /// ground. None has it walk on the spot.
    pub fn stride(&mut self, walked: Option<Stride>) {
        self.strode = walked;
    }

    /// Turns its head by `turn` and lifts it by `lift`, in radians,
    /// on top of whatever its motion does. Zero looks nowhere.
    pub fn look(&mut self, turn: f32, lift: f32) {
        let look = Vec2::new(turn, lift);
        if self.look != look {
            self.look = look;
        }
    }

    /// True when it is partway through something that will end.
    pub fn is_busy(&self, now: f32) -> bool {
        self.from.is_some()
            || self.act.length().is_some_and(|l| now - self.since < l)
    }

    fn start(&mut self, act: Act, now: f32) {
        self.from = Some((self.eval(now), self.act));
        self.act = act;
        self.since = now;
    }

    /// Takes up a motion as if it had always been in it.
    pub fn settle(&mut self, act: Act) {
        self.from = None;
        self.act = act;
        self.since = LONG_AGO;
        self.state = act.state();
        self.synced = self.state;
    }

    fn context(&self, now: f32) -> Context {
        Context {
            t: now * self.quirk.tempo + self.quirk.phase,
            tau: now - self.since,
            gait: self.gait,
            pace: self.pace,
            roll: ramp(0.0, 1.0, self.roll),
            q: self.quirk,
        }
    }

    /// The sound its motion makes, and a count that passes a whole
    /// number each time it makes it.
    pub fn beat(&self, now: f32) -> Option<(Sound, f32)> {
        acts::beat(self.act, &self.context(now))
    }

    fn eval(&self, now: f32) -> Pose {
        let c = self.context(now);
        let mut p = acts::pose(self.act, &c);
        if self.look != Vec2::ZERO {
            // The eyes go a little further than the head.
            p.head = ry(self.look.x) * p.head * rx(-self.look.y);
            p.gaze.x += 0.25 * self.look.x;
        }
        let Some((from, was)) = &self.from else {
            return p;
        };
        let (len, dip) = self.act.easing(*was);
        let u = (now - self.since) / len.max(1e-4);
        if u >= 1.0 {
            return p;
        }
        let mut p = from.mix(&p, ramp(0.0, 1.0, u));
        // A small dip between two motions, as a breath before the
        // next.
        let d = dip * arc(0.0, 1.0, u);
        p.hips.y -= 0.02 * d;
        p.squash *= 1.0 - 0.03 * d;
        p
    }
}
