//! The plain motion of each state, and walking.

use std::f32::consts::PI;

use glam::{Quat, Vec2};

use crate::keeper::acts::Ctx;
use crate::keeper::acts::parts::{
    arm_high, arm_l, blink, breathe, current, flicker, look_about,
    shift_weight, signal,
};
use crate::keeper::pose::{
    LIT, Pose, Prop, arc, moon, pulse, rx, ry, rz,
};

pub(super) fn idle(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    look_about(&mut p, c, 1.0);
    shift_weight(&mut p, c);
    p.swing = Vec2::new(
        0.025 * (c.t * 1.9).sin(),
        0.03 * (c.t * 1.3).sin(),
    );
    p.glow *= flicker(c.t);
    p
}

pub(super) fn reflect(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.3);
    blink(&mut p, c);
    let drift = (c.t * 0.45).sin();
    p.body = rx(-0.09);
    p.head = rz(0.08 * drift) * rx(-0.42) * ry(0.12 * drift);
    p.gaze.y = 0.012;
    p.lids *= 0.85;
    p.arm_l = arm_l(0.9, 1.15);
    p.arm_r = rx(-1.45);
    p.plant = 0.0;
    p.slide = 0.02;
    p.pole = rx(-0.06);
    p.swing = Vec2::new(0.0, 0.05 * (c.t * 0.9).sin());
    p.glow = moon(LIT * (0.55 + 0.2 * (c.t * 0.8).sin()));
    p
}

/// The purpose of a walk.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Gait {
    Plain,
    Letter,
    Amble,
    Pace,
}

pub(super) fn walk(c: &Ctx, kind: Gait) -> Pose {
    let mut p = Pose::stand();
    blink(&mut p, c);
    look_about(&mut p, c, 0.35);
    let a = c.pace;
    // The share of its pace it steps, and the share it rolls.
    let r = a * c.roll;
    let w = a - r;
    let (s, co) = c.gait.sin_cos();
    // Lowest just after each foot lands.
    let down = 0.5 - 0.5 * (2.0 * c.gait - 0.7).cos();
    // Rolling, the legs hold still with one foot a little ahead.
    p.leg_l = -0.55 * w * s - 0.17 * r;
    p.leg_r = 0.55 * w * s + 0.11 * r;
    p.lift_l = 0.07 * w * co.max(0.0).powf(1.5);
    p.lift_r = 0.07 * w * (-co).max(0.0).powf(1.5);
    p.wheels = c.roll;
    p.hips.y = -0.024 * w * down * c.q.bounce - 0.02 * r;
    p.hips.x = 0.022 * w * co * c.q.sway;
    p.squash = 1.0 + w * (0.012 - 0.035 * down * c.q.bounce);
    let balance = 0.03 * r * (c.t * 2.6).sin() * c.q.sway;
    p.body = rz(-0.075 * w * co * c.q.sway + balance)
        * ry(-0.16 * w * s)
        * rx(0.12 * a + 0.15 * r);
    // The head stays level and looks where it is going.
    p.head = p.head
        * ry(0.1 * w * s)
        * rz(0.05 * w * co * c.q.sway - balance)
        * rx(0.04 * w * (2.0 * c.gait - 1.4).sin()
            - 0.03 * a
            - 0.11 * r);
    p.head_lift = 0.006 * w * (2.0 * c.gait - 1.7).sin();
    p.hat = rx(-0.07 * r);
    p.hat_lift = 0.014 * w * (2.0 * c.gait - 2.3).sin().max(0.0);
    p.arm_l = rz(-0.14 - 0.1 * r)
        * rx(0.55 * w * (c.gait - 0.35).sin() + 0.5 * r);
    p.arm_r =
        rx(-1.05 - 0.2 * a - w * 0.05 * (2.0 * c.gait - 0.5).sin());
    p.plant = 1.0 - a;
    p.slide = 0.16;
    p.pole = rz(-0.06 * (1.0 - a))
        * rx(0.36 * a + 0.04 * w * (2.0 * c.gait - 0.9).sin());
    // The lantern lags the steps that jolt it, and trails a robot
    // that rolls.
    p.swing = Vec2::new(
        0.22 * w * (2.0 * c.gait - 1.5).sin() + 0.26 * r,
        0.14 * w * (c.gait - 1.1).sin(),
    );
    p.key = c.t * 2.0;
    p.flag = 0.35 * (c.t * 5.0).sin() * a;
    p.glow *= flicker(c.t);
    match kind {
        Gait::Plain => {}
        Gait::Letter => {
            p.arm_l = arm_l(
                0.35,
                1.2 + 0.07 * w * (2.0 * c.gait - 0.9).sin(),
            );
            p.prop = Prop::Letter;
            p.prop_size = 1.0;
            p.glow = current(LIT);
            p.ring = 1.0;
            p.key = c.t * 5.0;
        }
        Gait::Amble => {
            p.glow = current(LIT * 0.8);
            p.key = c.t * 5.0;
        }
        Gait::Pace => {
            // Hand to chin, eyes on the ground ahead.
            p.arm_l = arm_l(0.62, 2.3);
            p.head = p.head * rz(-0.13) * rx(0.2);
            p.gaze.y = -0.006;
            p.glow = current(LIT * (0.7 + 0.3 * (c.t * 2.4).sin()));
            p.key = c.t * 5.0;
        }
    }
    p
}

pub(super) fn wait(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 0.8);
    blink(&mut p, c);
    shift_weight(&mut p, c);
    // A small rise on the toes, to be noticed.
    let f = (c.t / 5.2).fract() * 5.2;
    let ahem = arc(0.3, 0.75, f);
    let wag = pulse(0.3, 0.45, 0.9, 1.1, f) * (c.t * 13.0).sin();
    p.shift.y = 0.03 * ahem;
    p.hat_lift = 0.02 * arc(0.5, 0.95, f);
    p.head = p.head * rz(0.06 * (c.t * 0.7).sin()) * rx(-0.2);
    p.head_lift += 0.05;
    p.gaze.y = 0.008;
    p.arm_l = arm_high(2.6 + 0.05 * (c.t * 1.1).sin() + 0.1 * wag)
        * rx(-0.25);
    p.arm_r = rx(-1.9);
    p.plant = 0.0;
    p.slide = 0.03;
    p.pole = Quat::IDENTITY;
    p.swing = Vec2::new(
        0.03 * (c.t * 1.1 - 1.0).sin(),
        0.05 * (c.t * 1.1 - 1.3).sin(),
    );
    p.prop = Prop::Warrant;
    p.prop_size = 1.0;
    p.glow = signal(LIT * (0.8 + 0.2 * (c.t * PI * 0.8).sin()));
    p
}
