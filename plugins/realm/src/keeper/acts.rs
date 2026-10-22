//! The motions. Each is a pure function of time, so any moment can be
//! drawn without having drawn the one before.

mod chores;
mod lives;
mod meeting;
mod parts;
mod plain;

use crate::keeper::KState;
use crate::keeper::acts::chores::{
    celebrate, crank, done, hammer, kite, read, recall, stow, tend,
    think, write,
};
use crate::keeper::acts::lives::{
    appear, leave, merge_host, merge_into,
};
use crate::keeper::acts::meeting::listen;
use crate::keeper::acts::plain::{Gait, idle, reflect, wait, walk};
use crate::keeper::pose::{Pose, dice};

/// One motion a robot can play.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    /// At home with nothing to do.
    Idle,
    /// Looking up, going over the day.
    Reflect,
    Walk,
    /// Walking with a letter.
    Carry,
    /// Listening at its door for an agent's answer.
    Listen,
    Think,
    /// Holding a request up for the human.
    Wait,
    /// Its first moments.
    Appear,
    /// Its last.
    Leave,
    /// Stepping into the robot in front of it.
    MergeInto,
    /// Taking in the robot in front of it.
    MergeHost,
    Celebrate,
    /// Walking about its own plot, on its way to a job.
    Amble,
    /// Walking slowly up and down, thinking.
    Pace,
    /// Sitting with its book of notes open.
    Read,
    /// Sitting and writing in its book of notes.
    Jot,
    /// Writing at its desk.
    Write,
    /// Working the wheel of its engine.
    Crank,
    /// Standing by while its engine runs.
    Tend,
    /// Flying a kite.
    Kite,
    /// Reading a scroll held up beside it.
    Recall,
    /// Rolling up a new scroll, stamping it and putting it by.
    Stow,
    /// Hammering at its bench.
    Tinker,
    /// Tipping its hat over finished work.
    Done,
}

/// Speed of a worked wheel, in radians a second.
pub const CRANK_RATE: f32 = 5.2;

/// Distance between two robots before they merge.
pub const MERGE_GAP: f32 = 0.9;

impl Act {
    /// The motion a state plays when nothing more specific is asked
    /// for.
    pub fn of(state: KState) -> Self {
        match state {
            KState::Rest => Self::Idle,
            KState::Working => Self::Carry,
            KState::Waiting => Self::Wait,
            // Nothing the agents service tells shows a fault.
            KState::Fault | KState::Failed => Self::Idle,
        }
    }

    pub fn state(self) -> KState {
        match self {
            Self::Idle
            | Self::Reflect
            | Self::Walk
            | Self::Appear
            | Self::Leave
            | Self::Celebrate
            | Self::Done => KState::Rest,
            Self::Carry
            | Self::Listen
            | Self::Think
            | Self::MergeInto
            | Self::MergeHost
            | Self::Amble
            | Self::Pace
            | Self::Read
            | Self::Jot
            | Self::Write
            | Self::Crank
            | Self::Tend
            | Self::Kite
            | Self::Recall
            | Self::Stow
            | Self::Tinker => KState::Working,
            Self::Wait => KState::Waiting,
        }
    }

    /// Seconds a motion takes when it plays once. None for one that
    /// repeats.
    pub fn length(self) -> Option<f32> {
        match self {
            Self::Appear => Some(3.2),
            Self::Leave => Some(3.6),
            Self::MergeInto => Some(1.8),
            Self::MergeHost => Some(2.9),
            Self::Celebrate => Some(2.5),
            Self::Stow => Some(3.4),
            Self::Done => Some(2.0),
            _ => None,
        }
    }

    /// The motion that follows a finished one by itself.
    pub fn then(self) -> Option<Self> {
        match self {
            Self::Appear
            | Self::MergeHost
            | Self::Celebrate
            | Self::Done => Some(Self::Idle),
            Self::Stow => Some(Self::Think),
            _ => None,
        }
    }

    /// True when the robot is out of sight once the motion has
    /// played.
    pub fn vanishes(self) -> bool {
        matches!(self, Self::Leave | Self::MergeInto)
    }

    pub fn walks(self) -> bool {
        matches!(
            self,
            Self::Walk | Self::Carry | Self::Amble | Self::Pace
        )
    }

    /// Seconds to ease into this motion from another, and how deep
    /// the dip that comes with the change.
    pub(super) fn easing(self, from: Self) -> (f32, f32) {
        match (from, self) {
            (_, Self::Appear) => (0.0, 0.0),
            (a, b) if a.walks() && b.walks() => (0.3, 0.0),
            _ => (0.45, 1.0),
        }
    }
}

/// The small differences in how one robot moves.
#[derive(Clone, Copy, Debug)]
pub(super) struct Quirk {
    pub seed: u32,
    /// Seconds its clock runs ahead of the world's.
    pub phase: f32,
    pub tempo: f32,
    pub bounce: f32,
    pub sway: f32,
}

impl Quirk {
    /// Seed 0 is the plain robot the others vary from.
    pub fn of(seed: u32) -> Self {
        if seed == 0 {
            return Self {
                seed,
                phase: 0.0,
                tempo: 1.0,
                bounce: 1.0,
                sway: 1.0,
            };
        }
        let d = |n: i32| dice(n, seed);
        Self {
            seed,
            phase: d(1) * 97.0,
            tempo: 0.9 + 0.2 * d(2),
            bounce: 0.8 + 0.45 * d(3),
            sway: 0.75 + 0.55 * d(4),
        }
    }
}

pub(super) struct Ctx {
    /// The robot's own clock.
    pub t: f32,
    /// Seconds since the motion began.
    pub tau: f32,
    /// Its place in its stride: a full turn is two steps.
    pub gait: f32,
    /// 0 standing still, 1 at full stride.
    pub pace: f32,
    /// 0 stepping, 1 rolling on its wheels.
    pub roll: f32,
    pub q: Quirk,
}

pub(super) fn pose(act: Act, c: &Ctx) -> Pose {
    match act {
        Act::Idle => idle(c),
        Act::Reflect => reflect(c),
        Act::Walk => walk(c, Gait::Plain),
        Act::Carry => walk(c, Gait::Letter),
        Act::Amble => walk(c, Gait::Amble),
        Act::Pace => walk(c, Gait::Pace),
        Act::Read => read(c, false),
        Act::Jot => read(c, true),
        Act::Write => write(c),
        Act::Crank => crank(c),
        Act::Tend => tend(c),
        Act::Kite => kite(c),
        Act::Recall => recall(c),
        Act::Stow => stow(c),
        Act::Tinker => hammer(c),
        Act::Done => done(c),
        Act::Listen => listen(c),
        Act::Think => think(c),
        Act::Wait => wait(c),
        Act::Appear => appear(c),
        Act::Leave => leave(c),
        Act::MergeInto => merge_into(c),
        Act::MergeHost => merge_host(c),
        Act::Celebrate => celebrate(c),
    }
}
