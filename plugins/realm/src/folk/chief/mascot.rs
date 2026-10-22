//! The chief in the world: a small cloaked figure with a face, a hat
//! and a wick of light that floats at its shoulder. Each of its
//! motions is a pure function of the scene's clock.

mod parts;
mod rig;

use std::f32::consts::{PI, TAU};

use glam::{Vec2, Vec3};

use crate::build::mesh::{hump, lerp, smoothstep};
use crate::folk::chief::look::Hat;
pub use crate::folk::chief::mascot::parts::{parts, wick};
pub use crate::folk::chief::mascot::rig::Rig;
use crate::folk::chief::wickstep::Phase;

/// Height of the middle of the head over the feet.
const HEAD: f32 = 1.06;

impl Hat {
    /// The wick's place over a head that wears it, from the figure's
    /// feet.
    pub fn crest(self) -> Vec3 {
        match self {
            Hat::Pointed => Vec3::new(0.14, HEAD + 1.12, -0.39),
            Hat::Hood => Vec3::new(0.0, HEAD + 0.6, -0.12),
            Hat::Crown => Vec3::new(0.0, HEAD + 0.62, 0.0),
            Hat::Wreath | Hat::Bare => {
                Vec3::new(0.0, HEAD + 0.56, 0.0)
            }
        }
    }
}

/// A mascot's motion.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Doing {
    /// Stands, breathes and looks about.
    #[default]
    Idle,
    Walk,
    /// Reads a letter held in both hands.
    Read,
    /// Bends to water the flowers, then stands back to look.
    Tend,
    /// Looks up at the sky, hands behind its back.
    Gaze,
    /// Stands by an agent and listens to it.
    Attend,
}

/// The place of every part of the rig at one moment.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pose {
    /// Height of the figure off the ground.
    pub lift: f32,
    /// Its width and its height, over those it is built at.
    pub girth: f32,
    pub height: f32,
    /// Its lean forward and to its left, in radians.
    pub lean: f32,
    pub roll: f32,
    /// The head's turn to its left, its nod down and its tilt.
    pub head: Vec3,
    /// 0 with the eyes open, 1 with them shut.
    pub blink: f32,
    /// Lift of the hat off the head, and its tip back.
    pub hat: Vec2,
    /// The left and the right hand.
    pub hands: [Vec3; 2],
    /// Offset of each foot from its place of rest.
    pub feet: [Vec3; 2],
    /// Forward tip of the thing held.
    pub item: f32,
    /// 1 with the letter in its hands, 0 with none.
    pub letter: f32,
    /// Offset of the wick from its place over the head.
    pub wick: Vec3,
    /// The wick's size and brightness, over those at rest.
    pub glow: f32,
}

impl Pose {
    pub fn toward(&self, to: &Pose, k: f32) -> Pose {
        Pose {
            lift: lerp(self.lift, to.lift, k),
            girth: lerp(self.girth, to.girth, k),
            height: lerp(self.height, to.height, k),
            lean: lerp(self.lean, to.lean, k),
            roll: lerp(self.roll, to.roll, k),
            head: self.head.lerp(to.head, k),
            blink: lerp(self.blink, to.blink, k),
            hat: self.hat.lerp(to.hat, k),
            hands: [
                self.hands[0].lerp(to.hands[0], k),
                self.hands[1].lerp(to.hands[1], k),
            ],
            feet: [
                self.feet[0].lerp(to.feet[0], k),
                self.feet[1].lerp(to.feet[1], k),
            ],
            item: lerp(self.item, to.item, k),
            letter: lerp(self.letter, to.letter, k),
            wick: self.wick.lerp(to.wick, k),
            glow: lerp(self.glow, to.glow, k),
        }
    }

    /// Standing at rest at clock time `t`: it breathes, it blinks,
    /// and its wick sways and flickers over its head.
    pub(super) fn rest(t: f32) -> Pose {
        let breath = (t * 1.9).sin();
        let turn = t % 3.7;
        let shut = hump(turn, 0.0, 0.16).max(
            hump(turn, 0.26, 0.4)
                * if ((t / 3.7) as u32).is_multiple_of(3) {
                    1.0
                } else {
                    0.0
                },
        );
        Pose {
            lift: 0.0,
            girth: 1.0 - 0.008 * breath,
            height: 1.0 + 0.016 * breath,
            lean: 0.0,
            roll: 0.0,
            head: Vec3::new(0.0, 0.02 * breath, 0.0),
            blink: shut,
            hat: Vec2::new(
                0.006 * breath,
                0.03 * (t * 1.9 - 0.6).sin(),
            ),
            hands: [
                Vec3::new(-0.36, 0.5 + 0.01 * breath, 0.1),
                Vec3::new(0.36, 0.5 + 0.01 * breath, 0.1),
            ],
            feet: [Vec3::ZERO; 2],
            item: 0.0,
            letter: 0.0,
            wick: Vec3::new(
                0.025 * (t * 0.9).sin(),
                0.03 * (t * 1.7).sin(),
                0.025 * (t * 0.9).cos(),
            ),
            glow: 1.0
                + 0.08 * (t * 11.3).sin()
                + 0.05 * (t * 17.1).sin(),
        }
    }

    /// The pose of `doing` at clock time `t`.
    pub fn of(doing: Doing, t: f32) -> Pose {
        let mut p = Pose::rest(t);
        match doing {
            Doing::Idle => {
                // Every so often it looks to one side and back, and
                // it shifts its weight all the while.
                let turn = t % 5.3;
                let side = if ((t / 5.3) as u32).is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                };
                p.head.x = 0.55 * side * hump(turn, 1.0, 3.2);
                p.roll = 0.035 * (t * 0.6).sin();
            }
            Doing::Walk => {
                let step = t * 2.8 * PI;
                let (s, hop) = (step.sin(), step.sin().abs());
                p.lift = 0.07 * hop;
                p.roll = 0.09 * s;
                p.lean = 0.08;
                p.feet = [
                    Vec3::new(0.0, 0.06 * s.max(0.0), 0.1 * s),
                    Vec3::new(0.0, 0.06 * (-s).max(0.0), -0.1 * s),
                ];
                p.hands[0].z -= 0.1 * s;
                p.hands[1].z += 0.1 * s;
                p.hat = Vec2::new(
                    0.035 * (step - 0.5).sin().abs(),
                    0.1 * hop,
                );
                p.wick += Vec3::new(0.0, 0.03 * hop, -0.1);
            }
            Doing::Read => {
                p.head = Vec3::new(
                    0.09 * (t * 1.3).sin(),
                    0.36 + 0.1 * hump(t % 4.0, 3.0, 3.8),
                    0.0,
                );
                p.hands = [
                    Vec3::new(-0.14, 0.72, 0.34),
                    Vec3::new(0.15, 0.7, 0.33),
                ];
                p.item = 0.5;
                p.letter = 1.0;
                // The wick leans over to light the page.
                p.wick += Vec3::new(0.0, -0.06, 0.2);
            }
            Doing::Tend => {
                let turn = (t % 5.0) / 5.0;
                let down = smoothstep(0.0, 0.12, turn)
                    * (1.0 - smoothstep(0.58, 0.7, turn));
                p.lean = 0.34 * down;
                p.head.y = 0.3 * down - 0.16 * (1.0 - down);
                p.hands[1] = p.hands[1].lerp(
                    Vec3::new(
                        0.3,
                        0.44 + 0.05 * (t * 6.0).sin(),
                        0.44,
                    ),
                    down,
                );
                p.item = 0.9 * down;
                p.lift = 0.08 * hump(turn, 0.76, 0.92);
            }
            Doing::Gaze => {
                p.head = Vec3::new(
                    0.2 * (t * 0.23).sin(),
                    -0.42,
                    0.03 * (t * 0.5).sin(),
                );
                p.hands = [
                    Vec3::new(-0.2, 0.48, -0.24),
                    Vec3::new(0.2, 0.48, -0.24),
                ];
                p.hat.y = 0.14;
                p.wick.y += 0.3;
            }
            Doing::Attend => {
                // It leans in with its head on one side and nods in
                // bouts, and now and then it lifts a hand to speak.
                let nod = (t * 2.1).sin().max(0.0).powi(2)
                    * hump(t % 6.5, 0.0, 3.2);
                p.lean = 0.06;
                p.head = Vec3::new(0.0, 0.12 * nod, 0.13);
                p.lift = 0.025 * nod;
                let says = hump(t % 6.5, 3.6, 5.4);
                p.hands[1] = p.hands[1].lerp(
                    Vec3::new(
                        0.34 + 0.04 * (t * 7.0).sin(),
                        0.82,
                        0.24,
                    ),
                    says,
                );
                p.item = -0.3 * says;
            }
        }
        p
    }

    /// The pose as a Wickstep bends it. Going out it crouches and is
    /// drawn up thin into its wick, which goes out. Coming in the
    /// wick catches, the figure pours down out of it and lands with
    /// a squash.
    pub(crate) fn stepping(mut self, phase: Phase, now: f32) -> Pose {
        match phase {
            Phase::Lit => {}
            Phase::Out { began, long } => {
                let u = ((now - began) / long).clamp(0.0, 1.0);
                let crouch = smoothstep(0.0, 0.3, u);
                let drawn = smoothstep(0.3, 1.0, u);
                self.girth *=
                    lerp(1.0 + 0.18 * crouch, 0.0, drawn.powf(1.5));
                self.height *= lerp(1.0 - 0.16 * crouch, 1.9, drawn);
                self.lift += 0.5 * drawn * drawn;
                self.wick.y += 0.1 * drawn - 0.12 * crouch;
                self.glow *= (1.0 + 1.5 * crouch)
                    * (1.0 - smoothstep(0.75, 1.0, u));
            }
            Phase::Dark { .. } => {
                self.girth = 0.0;
                self.glow = 0.0;
            }
            Phase::In { began, long } => {
                let u = ((now - began) / long).clamp(0.0, 1.0);
                let poured = smoothstep(0.08, 0.45, u);
                let squash = hump(u, 0.35, 0.72);
                let ring = if u > 0.62 {
                    0.07 * ((u - 0.62) * TAU * 3.2).sin() * (1.0 - u)
                } else {
                    0.0
                };
                self.girth *= poured * (1.0 + 0.22 * squash - ring);
                self.height *= lerp(1.9, 1.0, poured)
                    * (1.0 - 0.2 * squash + ring);
                self.lift += 0.5 * (1.0 - poured);
                self.wick.y -= 0.1 * squash;
                self.glow *= smoothstep(0.0, 0.12, u)
                    * (1.0 + 1.8 * (1.0 - smoothstep(0.1, 0.6, u)));
                self.hat.x += 0.12 * squash;
            }
        }
        self
    }
}
