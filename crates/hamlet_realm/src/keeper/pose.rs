//! A robot's pose: every channel the rig can move, and how two poses
//! mix.

use std::f32::consts::PI;

use bevy::prelude::*;
use hamlet_vfx::inlay::{self, EMBER};

pub(super) const HIP: f32 = 0.3;
pub(super) const NECK: f32 = 0.44;
pub(super) const SHOULDER: Vec3 = Vec3::new(0.27, 0.35, 0.0);
pub(super) const ARM: f32 = 0.3;
/// Hip to sole.
pub(super) const LEG: f32 = 0.3;
/// A wheel's radius.
pub(super) const WHEEL: f32 = 0.055;
/// Height above the sole at which a tucked wheel's rim is kept.
const TUCK: f32 = 0.014;
/// Height of a robot on its wheels above its soles.
const RIDE: f32 = 0.014;
/// The crook's foot below its origin.
pub(super) const POLE_FOOT: f32 = 0.42;

/// The thing in the free hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Prop {
    #[default]
    None,
    Warrant,
    Letter,
    Mallet,
    Block,
    Ballot,
    Book,
    Quill,
    Scroll,
    Kite,
}

impl Prop {
    /// Every prop that has a mesh.
    pub const HELD: [Self; 9] = [
        Self::Warrant,
        Self::Letter,
        Self::Mallet,
        Self::Block,
        Self::Ballot,
        Self::Book,
        Self::Quill,
        Self::Scroll,
        Self::Kite,
    ];
}

/// Angles in radians. The robot faces +Z; a positive pitch swings a
/// hanging limb back and leans the body forward.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pose {
    /// The whole figure, in its root's space.
    pub shift: Vec3,
    pub spin: Quat,
    pub size: Vec3,
    /// The pelvis: moves the body and the tops of the legs.
    pub hips: Vec3,
    pub body: Quat,
    /// The body's height; its width shrinks to keep the volume.
    pub squash: f32,
    pub head: Quat,
    pub head_lift: f32,
    pub hat: Quat,
    pub hat_lift: f32,
    /// The eyes' gaze: a turn about the head, and a lift.
    pub gaze: Vec2,
    /// Opening of each eye, left then right.
    pub lids: Vec2,
    pub wide: f32,
    pub arm_l: Quat,
    pub arm_r: Quat,
    pub leg_l: f32,
    pub leg_r: f32,
    /// Draw of each leg up into the body.
    pub lift_l: f32,
    pub lift_r: f32,
    /// 0 with the wheels tucked up beside the feet, 1 with them let
    /// down and carrying the robot.
    pub wheels: f32,
    /// 1 with the crook in the right hand, 0 with it let go.
    pub grip: f32,
    /// 1 with the held crook's foot on the ground.
    pub plant: f32,
    /// Height above the crook's origin at which the hand holds it
    /// when it is off the ground.
    pub slide: f32,
    pub pole: Quat,
    /// The crook's place when let go.
    pub pole_at: Vec3,
    /// 1 with the carried lantern hanging true, 0 with it stiff on
    /// the crook.
    pub hang: f32,
    /// The lantern's swing, fore and aft then side to side.
    pub swing: Vec2,
    pub prop: Prop,
    pub prop_size: f32,
    /// Slide of the prop's lit part beyond the hand.
    pub bead: f32,
    /// Colour times strength of both lights.
    pub glow: Vec3,
    pub ring: f32,
    /// The winding key's angle.
    pub key: f32,
    /// The mayor's pennant, as a turn about its spire.
    pub flag: f32,
}

pub(super) fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}

pub(super) fn ry(a: f32) -> Quat {
    Quat::from_rotation_y(a)
}

pub(super) fn rz(a: f32) -> Quat {
    Quat::from_rotation_z(a)
}

pub(super) fn candle(strength: f32) -> Vec3 {
    inlay::candle() * strength
}

impl Pose {
    /// Standing at ease, crook planted, candle lit.
    pub fn stand() -> Self {
        Self {
            shift: Vec3::ZERO,
            spin: Quat::IDENTITY,
            size: Vec3::ONE,
            hips: Vec3::ZERO,
            body: Quat::IDENTITY,
            squash: 1.0,
            head: Quat::IDENTITY,
            head_lift: 0.0,
            hat: Quat::IDENTITY,
            hat_lift: 0.0,
            gaze: Vec2::ZERO,
            lids: Vec2::ONE,
            wide: 1.0,
            arm_l: rz(-0.14),
            arm_r: rx(-1.05),
            leg_l: 0.0,
            leg_r: 0.0,
            lift_l: 0.0,
            lift_r: 0.0,
            wheels: 0.0,
            grip: 1.0,
            plant: 1.0,
            slide: 0.1,
            pole: rz(-0.06),
            pole_at: Vec3::new(0.3, POLE_FOOT, 0.26),
            hang: 1.0,
            swing: Vec2::ZERO,
            prop: Prop::None,
            prop_size: 0.0,
            bead: 0.0,
            glow: candle(EMBER * 1.4),
            ring: 0.0,
            key: 0.0,
            flag: 0.0,
        }
    }

    /// The pose `w` of the way from `self` to `to`. A prop that
    /// differs shrinks away before the new one grows.
    pub fn mix(&self, to: &Self, w: f32) -> Self {
        let f = |a: f32, b: f32| a + (b - a) * w;
        let (prop, prop_size) = if self.prop == to.prop {
            (to.prop, f(self.prop_size, to.prop_size))
        } else if w < 0.5 {
            (self.prop, self.prop_size * (1.0 - 2.0 * w))
        } else {
            (to.prop, to.prop_size * (2.0 * w - 1.0))
        };
        Self {
            shift: self.shift.lerp(to.shift, w),
            spin: self.spin.slerp(to.spin, w),
            size: self.size.lerp(to.size, w),
            hips: self.hips.lerp(to.hips, w),
            body: self.body.slerp(to.body, w),
            squash: f(self.squash, to.squash),
            head: self.head.slerp(to.head, w),
            head_lift: f(self.head_lift, to.head_lift),
            hat: self.hat.slerp(to.hat, w),
            hat_lift: f(self.hat_lift, to.hat_lift),
            gaze: self.gaze.lerp(to.gaze, w),
            lids: self.lids.lerp(to.lids, w),
            wide: f(self.wide, to.wide),
            arm_l: self.arm_l.slerp(to.arm_l, w),
            arm_r: self.arm_r.slerp(to.arm_r, w),
            leg_l: f(self.leg_l, to.leg_l),
            leg_r: f(self.leg_r, to.leg_r),
            lift_l: f(self.lift_l, to.lift_l),
            lift_r: f(self.lift_r, to.lift_r),
            wheels: f(self.wheels, to.wheels),
            grip: f(self.grip, to.grip),
            plant: f(self.plant, to.plant),
            slide: f(self.slide, to.slide),
            pole: self.pole.slerp(to.pole, w),
            pole_at: self.pole_at.lerp(to.pole_at, w),
            hang: f(self.hang, to.hang),
            swing: self.swing.lerp(to.swing, w),
            prop,
            prop_size,
            bead: f(self.bead, to.bead),
            glow: self.glow.lerp(to.glow, w),
            ring: f(self.ring, to.ring),
            key: f(self.key, to.key),
            flag: f(self.flag, to.flag),
        }
    }
}

/// The place of each part for a pose, in its parent's space.
pub(super) struct Placed {
    pub rig: Transform,
    pub body: Transform,
    pub head: Transform,
    pub hat: Transform,
    pub eye_l: Transform,
    pub eye_r: Transform,
    pub leg_l: Transform,
    pub leg_r: Transform,
    /// Each foot's axle, in its leg's space.
    pub axle: Vec3,
    pub pole: Transform,
    pub lamp: Quat,
}

/// Distance of a foot's axle ahead of the leg.
const AXLE: f32 = 0.035;
/// The centre of the head's glass above the neck.
pub(super) const FACE: f32 = 0.17;
/// The top of the head's glass, where a hat sits.
pub(super) const HAT_AT: f32 = FACE + 0.11;
/// Half the angle between the eyes, seen from the head's axis.
pub(super) const EYE_SPREAD: f32 = 0.39;
/// The point the carried lantern's chain meets the crook at.
pub(super) const HOOK: Vec3 = Vec3::new(0.0, 1.14, 0.2);

impl Pose {
    /// Works out the transforms. `size` is the robot's own scale.
    pub fn place(&self, size: f32) -> Placed {
        let squash = self.squash.max(0.2);
        let wide = squash.sqrt().recip();
        let stretch = Vec3::new(wide, squash, wide);
        let body_at = Vec3::Y * HIP + self.hips;

        // A leg draws up into the body before its foot would sink
        // into the ground.
        let wheels = self.wheels.clamp(0.0, 1.0);
        let leg = |x: f32, pitch: f32, lift: f32| {
            let top = HIP + self.hips.y + lift;
            let sunk = (LEG * pitch.cos() - top).max(0.0);
            Transform::from_xyz(
                x + self.hips.x,
                top + sunk + RIDE * wheels,
                self.hips.z,
            )
            .with_rotation(rx(pitch))
        };

        let hand = body_at
            + self.body
                * (stretch
                    * (SHOULDER + self.arm_r * (Vec3::NEG_Y * ARM)));
        let up = self.pole * Vec3::Y;
        // The height of the hand on the crook when its foot is on
        // the ground.
        let planted = if up.y > 0.2 {
            (hand.y - POLE_FOOT * up.y) / up.y
        } else {
            self.slide
        };
        let hold = self.slide + (planted - self.slide) * self.plant;
        let held = hand - up * hold;
        let pole_at = self.pole_at.lerp(held, self.grip);

        let eye = |side: f32, lid: f32| {
            Transform::from_xyz(0.0, FACE + self.gaze.y, 0.0)
                .with_rotation(ry(self.gaze.x + side * EYE_SPREAD))
                .with_scale(Vec3::new(
                    self.wide,
                    self.wide * lid.clamp(0.06, 1.3),
                    1.0,
                ))
        };

        Placed {
            rig: Transform {
                translation: self.shift * size,
                rotation: self.spin,
                scale: self.size.max(Vec3::splat(1e-3)) * size,
            },
            body: Transform {
                translation: body_at,
                rotation: self.body,
                scale: stretch,
            },
            head: Transform::from_xyz(
                0.0,
                NECK + self.head_lift,
                0.0,
            )
            .with_rotation(self.head),
            hat: Transform::from_xyz(
                0.0,
                HAT_AT + self.hat_lift,
                0.0,
            )
            .with_rotation(self.hat),
            eye_l: eye(-1.0, self.lids.x),
            eye_r: eye(1.0, self.lids.y),
            leg_l: leg(-0.11, self.leg_l, self.lift_l),
            leg_r: leg(0.11, self.leg_r, self.lift_r),
            axle: Vec3::new(
                0.0,
                WHEEL - LEG + TUCK - (TUCK + RIDE) * wheels,
                AXLE,
            ),
            pole: Transform::from_translation(pole_at)
                .with_rotation(self.pole),
            lamp: Quat::IDENTITY.slerp(
                self.pole.inverse()
                    * rz(self.swing.y)
                    * rx(self.swing.x),
                self.hang,
            ),
        }
    }
}

/// 0 to 1 across `a..b`, eased at both ends.
pub(super) fn ramp(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Rises across `a..b`, holds, and falls across `c..d`.
pub(super) fn pulse(a: f32, b: f32, c: f32, d: f32, x: f32) -> f32 {
    ramp(a, b, x) * (1.0 - ramp(c, d, x))
}

/// 0 to 1 with an overshoot before it settles.
pub(super) fn back_out(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0) - 1.0;
    1.0 + 2.9 * t * t * t + 1.9 * t * t
}

/// A knock that rings and dies away, from the moment `x` passes 0.
pub(super) fn ring(x: f32, rate: f32, decay: f32) -> f32 {
    if x < 0.0 {
        0.0
    } else {
        (-x * decay).exp() * (x * rate).cos()
    }
}

/// One arc of a sine across `a..b`, 0 outside.
pub(super) fn arc(a: f32, b: f32, x: f32) -> f32 {
    if x <= a || x >= b {
        0.0
    } else {
        (PI * (x - a) / (b - a)).sin()
    }
}

/// A repeatable number in 0..1 for a counter and a robot's seed.
pub(super) fn dice(n: i32, seed: u32) -> f32 {
    let mut h = (n as u32).wrapping_mul(0x9e3779b1)
        ^ seed.wrapping_mul(0x85ebca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a2d39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}
