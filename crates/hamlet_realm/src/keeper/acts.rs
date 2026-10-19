//! The motions. Each is a pure function of time, so any moment can be
//! drawn without having drawn the one before.

mod chores;
mod handled;
mod lives;
mod meeting;
mod parts;
mod plain;

use std::f32::consts::{PI, TAU};

use self::chores::{
    celebrate, crank, done, hammer, kite, lift, oops, read, recall,
    stow, tend, think, vote, write,
};
use self::handled::{dangle, land, startle, sulk, wiggle};
use self::lives::{appear, leave, merge_host, merge_into};
use self::meeting::{
    PASS, greet, hand_over, listen, receive, talk, wave,
};
use self::plain::{
    Gait, failed, fault, idle, object, reflect, sleep, wait, walk,
};
use super::KState;
use super::pose::{Pose, dice};
use super::work::RING_EVERY;
use crate::sound::Sound;

/// One motion a robot can play.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    /// At home with nothing to do.
    Idle,
    /// Asleep on its feet, leaning on the crook.
    Sleep,
    /// Looking up, going over the day.
    Reflect,
    Walk,
    /// Walking with a letter.
    Carry,
    HandOver,
    Receive,
    Talk,
    Listen,
    Think,
    Hammer,
    Lift,
    /// Holding a request up for the human.
    Wait,
    /// Arguing against an instruction while it waits for the reply.
    Object,
    Fault,
    Failed,
    /// Its first moments.
    Appear,
    /// Its last.
    Leave,
    /// Stepping into the robot in front of it.
    MergeInto,
    /// Taking in the robot in front of it.
    MergeHost,
    Celebrate,
    Vote,
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
    /// Startled by something that went wrong.
    Oops,
    /// Tipping its hat over finished work.
    Done,
    /// Poked: a start, a look up at whoever did it, and a wave.
    Startle,
    /// Poked once too often: arms crossed, and a huff.
    Sulk,
    /// Patted: a happy wiggle.
    Wiggle,
    /// Held up off the ground, kicking.
    Dangle,
    /// Set down: a bounce, and a shake of the head.
    Land,
    /// Meeting another robot: a wave and a bow.
    Greet,
    /// A wave to whoever is passing.
    Wave,
}

/// Distance of a robot's seat behind its standing place.
pub const SEAT_BACK: f32 = 0.3;
/// Speed of a worked wheel, in radians a second.
pub const CRANK_RATE: f32 = 5.2;

/// Distance between two robots that stand face to face, to talk or to
/// pass a letter.
pub const TALK_GAP: f32 = 0.86;
/// Distance between two robots before they merge.
pub const MERGE_GAP: f32 = 0.9;

impl Act {
    pub const ALL: [Self; 42] = [
        Self::Idle,
        Self::Sleep,
        Self::Reflect,
        Self::Walk,
        Self::Carry,
        Self::HandOver,
        Self::Receive,
        Self::Talk,
        Self::Listen,
        Self::Think,
        Self::Hammer,
        Self::Lift,
        Self::Wait,
        Self::Object,
        Self::Fault,
        Self::Failed,
        Self::Appear,
        Self::Leave,
        Self::MergeInto,
        Self::MergeHost,
        Self::Celebrate,
        Self::Vote,
        Self::Amble,
        Self::Pace,
        Self::Read,
        Self::Jot,
        Self::Write,
        Self::Crank,
        Self::Tend,
        Self::Kite,
        Self::Recall,
        Self::Stow,
        Self::Tinker,
        Self::Oops,
        Self::Done,
        Self::Startle,
        Self::Sulk,
        Self::Wiggle,
        Self::Dangle,
        Self::Land,
        Self::Greet,
        Self::Wave,
    ];

    /// The motion a state plays when nothing more specific is asked
    /// for.
    pub fn of(state: KState) -> Self {
        match state {
            KState::Rest => Self::Idle,
            KState::Working => Self::Carry,
            KState::Waiting => Self::Wait,
            KState::Fault => Self::Fault,
            KState::Failed => Self::Failed,
        }
    }

    pub fn state(self) -> KState {
        match self {
            Self::Idle
            | Self::Sleep
            | Self::Reflect
            | Self::Walk
            | Self::Appear
            | Self::Leave
            | Self::Celebrate
            | Self::Vote
            | Self::Done
            | Self::Startle
            | Self::Sulk
            | Self::Wiggle
            | Self::Dangle
            | Self::Land
            | Self::Greet
            | Self::Wave => KState::Rest,
            Self::Carry
            | Self::HandOver
            | Self::Receive
            | Self::Talk
            | Self::Listen
            | Self::Think
            | Self::Hammer
            | Self::Lift
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
            | Self::Tinker
            | Self::Oops => KState::Working,
            Self::Wait | Self::Object => KState::Waiting,
            Self::Fault => KState::Fault,
            Self::Failed => KState::Failed,
        }
    }

    /// Seconds a motion takes when it plays once. None for one that
    /// repeats.
    pub fn length(self) -> Option<f32> {
        match self {
            Self::HandOver => Some(1.9),
            Self::Receive => Some(1.9),
            Self::Failed => Some(2.6),
            Self::Appear => Some(3.2),
            Self::Leave => Some(3.6),
            Self::MergeInto => Some(1.8),
            Self::MergeHost => Some(2.9),
            Self::Celebrate => Some(2.5),
            Self::Vote => Some(3.3),
            Self::Stow => Some(3.4),
            Self::Oops => Some(1.9),
            Self::Done => Some(2.0),
            Self::Startle => Some(2.4),
            Self::Sulk => Some(3.6),
            Self::Wiggle => Some(1.5),
            Self::Land => Some(0.9),
            Self::Greet => Some(2.0),
            Self::Wave => Some(1.6),
            _ => None,
        }
    }

    /// The motion that follows a finished one by itself.
    pub fn then(self) -> Option<Self> {
        match self {
            Self::HandOver
            | Self::Appear
            | Self::MergeHost
            | Self::Celebrate
            | Self::Vote
            | Self::Done
            | Self::Startle
            | Self::Sulk
            | Self::Wiggle
            | Self::Land
            | Self::Greet
            | Self::Wave => Some(Self::Idle),
            Self::Stow | Self::Oops => Some(Self::Think),
            _ => None,
        }
    }

    /// True when the robot is out of sight once the motion has
    /// played.
    pub(crate) fn vanishes(self) -> bool {
        matches!(self, Self::Leave | Self::MergeInto)
    }

    /// True when a robot settled in this motion can go undrawn. It
    /// still breathes when frames come for another reason.
    pub fn rests(self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Sleep | Self::Reflect | Self::Failed
        )
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
            // A fault is a knock: it comes at once.
            (_, Self::Fault) => (0.14, 0.0),
            (_, Self::Oops | Self::Startle) => (0.1, 0.0),
            // A hand takes it and lets it go at once.
            (_, Self::Dangle) => (0.18, 0.0),
            (_, Self::Land) => (0.06, 0.0),
            (Self::Talk, Self::Listen)
            | (Self::Listen, Self::Talk) => (0.4, 0.2),
            (a, b) if a.walks() && b.walks() => (0.3, 0.0),
            (_, Self::Failed) => (0.3, 0.0),
            (Self::Sleep, _) | (_, Self::Sleep) => (0.9, 0.3),
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
        Act::Sleep => sleep(c),
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
        Act::Tinker => hammer(c, true),
        Act::Oops => oops(c),
        Act::Done => done(c),
        Act::HandOver => hand_over(c),
        Act::Receive => receive(c),
        Act::Talk => talk(c),
        Act::Listen => listen(c),
        Act::Think => think(c),
        Act::Hammer => hammer(c, false),
        Act::Lift => lift(c),
        Act::Wait => wait(c),
        Act::Object => object(c),
        Act::Fault => fault(c),
        Act::Failed => failed(c),
        Act::Appear => appear(c),
        Act::Leave => leave(c),
        Act::MergeInto => merge_into(c),
        Act::MergeHost => merge_host(c),
        Act::Celebrate => celebrate(c),
        Act::Vote => vote(c),
        Act::Startle => startle(c),
        Act::Sulk => sulk(c),
        Act::Wiggle => wiggle(c),
        Act::Dangle => dangle(c),
        Act::Land => land(c),
        Act::Greet => greet(c),
        Act::Wave => wave(c),
    }
}

/// The sound a motion makes, and a count that passes a whole number
/// each time it makes it.
pub(super) fn beat(act: Act, c: &Ctx) -> Option<(Sound, f32)> {
    let steps = c.gait / PI - 0.5;
    let once = |at: f32| f32::from(c.tau >= at);
    Some(match act {
        // A robot on its wheels takes no steps.
        _ if act.walks() && (c.pace <= 0.3 || c.roll > 0.5) => {
            return None;
        }
        Act::Walk | Act::Amble | Act::Pace => (Sound::Step, steps),
        Act::Carry => (Sound::RoadStep, steps),
        Act::Hammer | Act::Tinker => {
            (Sound::Hammer, c.t / 0.95 - 0.68)
        }
        Act::Crank => (Sound::Wheel, c.tau * CRANK_RATE / TAU),
        Act::Tend => (Sound::Steam, c.tau / RING_EVERY - 0.05),
        Act::Write => {
            // Some lines it dips the quill or reads the page over.
            let line = c.t / 1.9;
            if dice(line.floor() as i32 + 419, c.q.seed) < 0.36 {
                return None;
            }
            (Sound::Quill, line - 0.06)
        }
        Act::Jot => (Sound::Quill, c.t / 1.9),
        Act::Read => {
            let span = c.t / 4.4;
            if dice(span.floor() as i32 + 211, c.q.seed) < 0.24 {
                return None;
            }
            (Sound::Letter, span - 0.03)
        }
        Act::Kite => (Sound::Kite, ((c.tau - 1.2) / 3.4).max(-0.5)),
        Act::Stow => (Sound::Hammer, once(2.05)),
        Act::HandOver => (Sound::Letter, once(PASS)),
        // A door for one that comes into the world or goes out of it.
        Act::Appear | Act::Leave | Act::MergeInto => {
            (Sound::Door, once(0.2))
        }
        _ => return None,
    })
}
