//! A robot's chores while its agent works alone
//! (docs/working-alone.md).

use hamlet_engine_sdk::api::agents::{Doing, Paper};

use crate::keeper::acts::Act;
use crate::keeper::pose::dice;
use crate::keeper::trip::Stride;
use crate::yard::{BEAT, Station};

/// One thing a robot does about its own house.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chore {
    /// At its door with nothing to do.
    Home,
    /// Held where it is by its state: waiting for the chief.
    Stand,
    /// Thinking, up and down its yard.
    Ponder,
    /// Reading its notes.
    Study,
    /// Writing its notes.
    Jot,
    /// Writing a file.
    Write,
    /// Running a command on its engine.
    Run,
    /// Fetching from the network, by kite.
    Fetch,
    /// Reading one of its skills.
    Recall,
    /// Saving a new skill.
    Stow,
    /// Using a tool added by hand.
    Tinker,
    Reflect,
    /// Its work is finished.
    Done,
    /// Listening at its door for an agent's answer.
    Await,
}

/// A robot's place on its beat.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Beat {
    /// 0 at one end, 1 at the other.
    pub at: f32,
    /// 1 walking toward the far end, -1 walking back, 0 standing.
    pub heading: f32,
    /// Ground covered in this walk.
    pub gone: f32,
    pub pace: f32,
    /// Ground covered per second.
    pub speed: f32,
}

impl Beat {
    pub fn stride(&self) -> Stride {
        Stride {
            gone: self.gone,
            pace: self.pace,
            speed: self.speed,
        }
    }
}

/// The motion a chore is in at one moment.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bit {
    pub act: Act,
    /// Seconds after reaching the station that the motion began.
    pub since: f32,
    pub beat: Option<Beat>,
}

/// Ground covered per second on the beat.
const PACE_SPEED: f32 = 0.5;
/// Seconds a command runs before the robot stops working the wheel
/// and stands by.
const LONG_RUN: f32 = 6.0;

impl Chore {
    pub fn of(doing: Doing) -> Self {
        match doing {
            Doing::Unknown | Doing::Think => Self::Ponder,
            Doing::Read(Paper::Skill) => Self::Recall,
            Doing::Read(_) => Self::Study,
            Doing::Write(Paper::File) => Self::Write,
            Doing::Write(Paper::Notes) => Self::Jot,
            Doing::Write(Paper::Skill) => Self::Stow,
            Doing::Run => Self::Run,
            Doing::Fetch => Self::Fetch,
            Doing::CustomTool => Self::Tinker,
            Doing::Await => Self::Await,
        }
    }

    pub fn station(self) -> Station {
        match self {
            Self::Home | Self::Await => Station::Porch,
            Self::Ponder => Station::Beat,
            Self::Study
            | Self::Jot
            | Self::Write
            | Self::Stow
            | Self::Tinker => Station::Desk,
            Self::Recall => Station::Rack,
            Self::Run => Station::Engine,
            Self::Fetch | Self::Reflect => Station::Open,
            Self::Stand | Self::Done => Station::Here,
        }
    }

    /// Seconds a chore is shown for once the robot is at its
    /// station, however soon the agent moves on.
    pub fn least(self) -> f32 {
        match self {
            Self::Home | Self::Stand | Self::Ponder | Self::Await => {
                0.0
            }
            Self::Stow => 3.4,
            Self::Done => 2.5,
            _ => 2.4,
        }
    }

    /// True when it cuts short whatever the robot is doing.
    pub fn breaks_in(self) -> bool {
        matches!(self, Self::Stand | Self::Done)
    }

    /// The motion `x` seconds after reaching the station. `ran` is
    /// how long the step that asked for the chore has been going,
    /// and `turn` a number that differs from one occasion to the
    /// next.
    pub fn bit(self, x: f32, ran: f32, seed: u32, turn: u32) -> Bit {
        let plain = |act| Bit {
            act,
            since: 0.0,
            beat: None,
        };
        match self {
            Self::Home | Self::Stand => plain(Act::Idle),
            Self::Await => plain(Act::Listen),
            Self::Ponder => ponder(x, seed),
            Self::Study => plain(Act::Read),
            Self::Jot => plain(Act::Jot),
            Self::Write => plain(Act::Write),
            Self::Run => {
                let began = x - ran.clamp(0.0, x.max(0.0));
                if x - began < LONG_RUN {
                    Bit {
                        act: Act::Crank,
                        since: began,
                        beat: None,
                    }
                } else {
                    Bit {
                        act: Act::Tend,
                        since: began + LONG_RUN,
                        beat: None,
                    }
                }
            }
            Self::Fetch => plain(Act::Kite),
            Self::Recall => plain(Act::Recall),
            Self::Stow => plain(Act::Stow),
            Self::Tinker => plain(Act::Tinker),
            Self::Reflect => plain(Act::Reflect),
            Self::Done => {
                // Most work ends with a tip of the hat, some with a
                // dance.
                if dice(turn as i32 + 733, seed) < 0.3 {
                    plain(Act::Celebrate)
                } else {
                    plain(Act::Done)
                }
            }
        }
    }
}

/// Thinking: a walk to another stop on the beat, then a while stood
/// there, over and over.
fn ponder(x: f32, seed: u32) -> Bit {
    const SPAN: f32 = 9.0;
    const STOPS: [f32; 4] = [0.0, 0.3, 0.7, 1.0];
    // Even spans end in the near half and odd ones in the far half,
    // so every walk crosses the middle.
    let stop = |n: i32| {
        let half = n.rem_euclid(2) as usize * 2;
        STOPS[half + usize::from(dice(n + 31, seed) < 0.5)]
    };
    let n = (x / SPAN).floor();
    let f = x - n * SPAN;
    let n = n as i32;
    let (from, to) = (stop(n - 1), stop(n));
    let length = (to - from).abs() * BEAT;
    let walk = length / PACE_SPEED;
    let start = n as f32 * SPAN;
    if f < walk {
        let k = f / walk;
        let eased = k * k * (3.0 - 2.0 * k);
        Bit {
            act: Act::Pace,
            since: start,
            beat: Some(Beat {
                at: from + (to - from) * eased,
                heading: (to - from).signum(),
                gone: length * eased,
                pace: (4.0 * k * (1.0 - k)).sqrt().min(1.0),
                speed: PACE_SPEED * 6.0 * k * (1.0 - k),
            }),
        }
    } else {
        Bit {
            act: Act::Think,
            since: start + walk,
            beat: Some(Beat {
                at: to,
                heading: 0.0,
                gone: 0.0,
                pace: 0.0,
                speed: 0.0,
            }),
        }
    }
}

/// The chore of a robot whose agent is busy with nothing more known,
/// and the clock time it began: a round of a think and then a tool
/// that depends only on its seed and the clock.
pub fn routine(seed: u32, t: f32) -> (Chore, f32) {
    const BLOCK: f32 = 18.0;
    let late = dice(977, seed) * BLOCK * 16.0;
    let n = ((t + late) / BLOCK).floor();
    let start = n * BLOCK - late;
    let n = n as i32;
    let think = 4.0 + 4.0 * dice(n * 3 + 1, seed);
    if t - start < think {
        return (Chore::Ponder, start);
    }
    let r = dice(n * 3 + 2, seed);
    let tool = if r < 0.4 {
        Chore::Run
    } else if r < 0.62 {
        Chore::Write
    } else if r < 0.74 {
        Chore::Study
    } else if r < 0.84 {
        Chore::Fetch
    } else if r < 0.91 {
        Chore::Recall
    } else if r < 0.96 {
        Chore::Jot
    } else {
        Chore::Reflect
    };
    (tool, start + think)
}
