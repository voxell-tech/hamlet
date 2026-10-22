//! A walk along a line of points, and how far along it a robot has
//! come.

use std::f32::consts::TAU;

use glam::Vec3;

use crate::keeper::Motion;

/// The ground a robot that is moved has covered, and its speed.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Stride {
    /// Ground covered so far.
    pub gone: f32,
    /// Its speed as a share of the speed it reaches.
    pub pace: f32,
    /// Ground covered per second.
    pub speed: f32,
}

/// A walk along a line of points, begun at a clock time. A robot
/// with one is moved along it and steps in time with the ground it
/// covers.
#[derive(Clone, Debug)]
pub struct Trip {
    pub way: Vec<Vec3>,
    pub depart: f32,
    /// Ground covered per second once under way.
    pub speed: f32,
    /// Seconds it takes to reach its speed, and to stop.
    pub get_going: f32,
    /// Distance either side of a corner over which the turn is
    /// spread.
    pub corner: f32,
}

/// The point a [`Trip`] has got to.
pub struct Step {
    pub at: Vec3,
    pub yaw: f32,
    /// Ground covered so far.
    pub gone: f32,
    /// Its speed as a share of the speed it reaches.
    pub pace: f32,
    /// Ground covered per second.
    pub speed: f32,
}

impl Step {
    pub fn stride(&self) -> Stride {
        Stride {
            gone: self.gone,
            pace: self.pace,
            speed: self.speed,
        }
    }
}

impl Trip {
    pub fn new(way: Vec<Vec3>, depart: f32, motion: &Motion) -> Self {
        Self {
            way,
            depart,
            speed: motion.walk_speed,
            get_going: motion.get_going,
            corner: motion.corner,
        }
    }

    pub fn length(&self) -> f32 {
        self.way.windows(2).map(|w| w[0].distance(w[1])).sum()
    }

    /// The speed it reaches: lower than asked on a walk too short to
    /// get going.
    fn cruise(&self, length: f32) -> f32 {
        self.speed.min(length / self.get_going).max(1e-4)
    }

    /// Seconds from setting off to standing still.
    pub fn duration(&self) -> f32 {
        let length = self.length();
        length / self.cruise(length) + self.get_going
    }

    pub fn at(&self, now: f32) -> Step {
        let length = self.length();
        let v = self.cruise(length);
        let total = length / v + self.get_going;
        let x = (now - self.depart).clamp(0.0, total);
        let (gone, speed) = if x < self.get_going {
            (
                v * x * x / (2.0 * self.get_going),
                v * x / self.get_going,
            )
        } else if x < total - self.get_going {
            (v * (x - self.get_going / 2.0), v)
        } else {
            let left = total - x;
            (
                length - v * left * left / (2.0 * self.get_going),
                v * left / self.get_going,
            )
        };

        let yaw =
            |w: &[Vec3]| (w[1].x - w[0].x).atan2(w[1].z - w[0].z);
        let mut from = 0.0;
        let mut found = None;
        for (i, w) in self.way.windows(2).enumerate() {
            let len = w[0].distance(w[1]);
            if gone <= from + len || i + 2 == self.way.len() {
                found = Some((i, from, len));
                break;
            }
            from += len;
        }
        let Some((i, from, len)) = found else {
            let at = self.way.first().copied().unwrap_or_default();
            return Step {
                at,
                yaw: 0.0,
                gone: 0.0,
                pace: 0.0,
                speed: 0.0,
            };
        };
        let w = &self.way[i..i + 2];
        let k = ((gone - from) / len.max(1e-5)).clamp(0.0, 1.0);
        let mut heading = yaw(w);
        // Turn through a corner across a short stretch either side.
        let into = gone - from;
        let out = from + len - gone;
        if i > 0 && into < self.corner {
            let before = yaw(&self.way[i - 1..i + 1]);
            heading =
                turn(before, heading, 0.5 + 0.5 * into / self.corner);
        } else if i + 2 < self.way.len() && out < self.corner {
            let after = yaw(&self.way[i + 1..i + 3]);
            heading =
                turn(heading, after, 0.5 - 0.5 * out / self.corner);
        }
        Step {
            at: w[0].lerp(w[1], k),
            yaw: heading,
            gone,
            pace: speed / self.speed.max(1e-4),
            speed,
        }
    }
}

/// The angle `w` of the way from `a` to `b`, the short way round.
pub(super) fn turn(a: f32, b: f32, w: f32) -> f32 {
    let d = (b - a + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0;
    a + d * w
}
