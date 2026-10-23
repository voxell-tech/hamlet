//! The looks that tell one robot from another.

use crate::folk::keeper::pose::dice;
use crate::society::AgentId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hat {
    /// The leader's brass dome, with a pennant.
    Dome,
    Cap,
    Cone,
    Bowler,
    Pipe,
    Knit,
    Straw,
}

impl Hat {
    /// The hats an ordinary robot can wear.
    pub(crate) const COMMON: [Self; 6] = [
        Self::Cap,
        Self::Cone,
        Self::Bowler,
        Self::Pipe,
        Self::Knit,
        Self::Straw,
    ];
}

/// The colour a robot's body and hat are painted. Paint is matte and
/// never one of the hues a light can show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Paint {
    Wood,
    Moss,
    Slate,
    Plum,
    Charcoal,
    Sage,
    Navy,
}

impl Paint {
    pub const ALL: [Self; 7] = [
        Self::Wood,
        Self::Moss,
        Self::Slate,
        Self::Plum,
        Self::Charcoal,
        Self::Sage,
        Self::Navy,
    ];
}

/// The mark on a robot's chest plate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Emblem {
    /// The leader's.
    Keyhole,
    Dot,
    Bars,
    Diamond,
    Plus,
    Chevron,
}

impl Emblem {
    pub(crate) const COMMON: [Self; 5] = [
        Self::Dot,
        Self::Bars,
        Self::Diamond,
        Self::Plus,
        Self::Chevron,
    ];
}

/// One robot's differences from the rest.
#[derive(Clone, Copy, Debug)]
pub struct Who {
    /// Sets its timing and its manner. 0 is the plain robot.
    pub seed: u32,
    pub hat: Hat,
    pub paint: Paint,
    pub emblem: Emblem,
    pub size: f32,
    /// Wears the cream mantle and flies a pennant.
    pub leader: bool,
}

impl Who {
    /// The robot of `agent`, which leads its town when `leader`.
    pub fn of(agent: AgentId, leader: bool) -> Self {
        match leader {
            true => Self::leader(),
            false => Self::citizen(agent.0 * 7 + 3),
        }
    }

    pub fn leader() -> Self {
        Self {
            seed: 0,
            hat: Hat::Dome,
            paint: Paint::Wood,
            emblem: Emblem::Keyhole,
            size: 1.12,
            leader: true,
        }
    }

    /// An ordinary robot whose looks all follow from `seed`.
    pub fn citizen(seed: u32) -> Self {
        let seed = seed.max(1);
        let pick = |n: i32, len: usize| {
            ((dice(n, seed) * len as f32) as usize).min(len - 1)
        };
        Self {
            seed,
            hat: Hat::COMMON[pick(21, Hat::COMMON.len())],
            paint: Paint::ALL[pick(22, Paint::ALL.len())],
            emblem: Emblem::COMMON[pick(23, Emblem::COMMON.len())],
            size: 0.84 + 0.18 * dice(24, seed),
            leader: false,
        }
    }
}
