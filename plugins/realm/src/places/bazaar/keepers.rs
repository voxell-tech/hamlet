//! The keepers of the stalls, each a figure in a look of its own,
//! who keep to their trades, and turn and wave to the chief who
//! chooses their stall or buys from it.

mod dress;

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec3};

use crate::build::frame::Frame;
use crate::build::mesh::smoothstep;
use crate::folk::chief::look::{Cloak, Hat, Holds, Light, Look};
use crate::folk::chief::mascot::{Doing, Pose};
use crate::places::bazaar::build::{KEEPER, at_work, placing};
use crate::places::bazaar::figure::Figure;
pub use crate::places::bazaar::keepers::dress::worn;
use crate::places::bazaar::{BazaarSettings, Stall};

/// Each keeper's dress, in the order of [`Stall::ALL`].
pub const LOOKS: [Look; 5] = [
    Look {
        cloak: Cloak::Plum,
        light: Light::Rune,
        hat: Hat::Bare,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Ember,
        light: Light::Candle,
        hat: Hat::Bare,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Slate,
        light: Light::Leaf,
        hat: Hat::Bare,
        holds: Holds::Nothing,
    },
    Look {
        cloak: Cloak::Moss,
        light: Light::Moon,
        hat: Hat::Hood,
        holds: Holds::Quill,
    },
    Look {
        cloak: Cloak::Rose,
        light: Light::Rosy,
        hat: Hat::Bare,
        holds: Holds::Book,
    },
];

/// The way a keeper faces when it is not at its work: out over the
/// plaza, a little toward its counter.
const REST: f32 = -0.3;

/// The keeper of a stall.
pub struct Keeper {
    pub stall: Stall,
    pub figure: Figure,
    /// The clock time it was called to, while it is.
    called: Option<f32>,
    /// The way it faces, as a rotation about Y from +Z.
    facing: f32,
    /// What it does, the pose it was last drawn in, the pose it
    /// eases from and the clock time that began.
    doing: Doing,
    shown: Pose,
    from: Pose,
    changed: f32,
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
    if t.rem_euclid(long) < working {
        (work, true)
    } else {
        (rest, false)
    }
}

/// The keeper's own clock: each a few seconds apart, so that no two
/// change together.
fn own(stall: Stall, t: f32) -> f32 {
    t + stall as usize as f32 * 3.7
}

/// True when the keeper of `stall` is at its work at clock time `t`.
pub fn working(stall: Stall, t: f32) -> bool {
    habit(stall, own(stall, t)).1
}

/// `from` turned toward `to` by no more than `most`, the short way
/// round.
fn turned(from: f32, to: f32, most: f32) -> f32 {
    let gap = (to - from + PI).rem_euclid(TAU) - PI;
    from + gap.clamp(-most, most)
}

impl Keeper {
    pub fn new(stall: Stall, figure: Figure) -> Self {
        let pose = Pose::of(Doing::Idle, 0.0);
        Self {
            stall,
            figure,
            called: None,
            facing: REST,
            doing: Doing::Idle,
            shown: pose,
            from: pose,
            changed: f32::MIN,
        }
    }

    /// Keeps to its trade, or turns to `eye`, given in the bazaar's
    /// frame, and waves while `called` or `cheering`, then listens.
    /// It stands at its stall `dt` seconds after it last did, at
    /// clock time `now`, `size` times the size it is built at.
    pub fn tend(
        &mut self,
        set: &BazaarSettings,
        (now, dt): (f32, f32),
        (called, cheering): (bool, bool),
        eye: Vec3,
        size: f32,
    ) {
        let stall = self.stall;
        self.called = match called || cheering {
            true => self.called.or(Some(now)),
            false => None,
        };
        let (at, turn) = placing(stall);
        let t = own(stall, now);
        let (doing, yaw) = match self.called {
            Some(since) => {
                let way = turn.inverse() * (eye - at) - KEEPER;
                let doing = match cheering || now - since < set.waves
                {
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
        self.facing = turned(self.facing, yaw, set.turn * dt);
        if self.doing != doing {
            self.from = self.shown;
            self.doing = doing;
            self.changed = now;
        }
        let eased = smoothstep(0.0, set.blend, now - self.changed);
        let pose = self.from.toward(&Pose::of(doing, t), eased);
        self.shown = pose;
        let stands = Frame::at(at + turn * KEEPER)
            .with_turn(turn * Quat::from_rotation_y(self.facing));
        self.figure.place(stands, &pose, size);
    }
}
