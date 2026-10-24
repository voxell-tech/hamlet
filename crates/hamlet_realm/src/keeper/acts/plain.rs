//! The plain motion of each state, and walking.

use std::f32::consts::PI;

use bevy::prelude::*;
use hamlet_vfx::inlay::{self, EMBER, LIT};

use super::Context;
use super::parts::{
    arm_high, arm_l, blink, breathe, current, flicker, look_about,
    moon, shift_weight, signal,
};
use crate::keeper::pose::{
    POLE_FOOT, Pose, Prop, arc, candle, dice, pulse, ramp, ring, rx,
    ry, rz,
};

pub(super) fn idle(c: &Context) -> Pose {
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

pub(super) fn sleep(c: &Context) -> Pose {
    let mut p = Pose::stand();
    let b = (c.t * 0.9).sin();
    // The head sinks, then starts awake.
    const EVERY: f32 = 6.5;
    let f = (c.t / EVERY).fract();
    let nod = ramp(0.0, 0.88, f) - ramp(0.88, 0.95, f);
    p.squash = 1.0 + 0.028 * b;
    p.hips.x = 0.025;
    p.body = rz(-0.1) * rx(0.08);
    p.head = rz(-0.32) * rx(0.14 + 0.03 * b + 0.16 * nod);
    p.head_lift = -0.012 + 0.006 * b;
    p.hat = rx(0.08 + 0.05 * nod);
    p.lids = Vec2::splat(0.08);
    p.arm_l = rz(-0.06);
    p.arm_r = rx(-1.3);
    p.pole = rz(0.1);
    p.swing = Vec2::new(0.0, 0.02 * (c.t * 0.9 - 1.0).sin());
    p.glow = candle(EMBER * (0.45 + 0.2 * b));
    p
}

pub(super) fn reflect(c: &Context) -> Pose {
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

pub(super) fn walk(c: &Context, kind: Gait) -> Pose {
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

pub(super) fn wait(c: &Context) -> Pose {
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

pub(super) fn object(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    // The crook comes up and down once, hard.
    let raise = pulse(0.0, 0.3, 0.38, 0.46, c.tau);
    let thump = ring(c.tau - 0.46, 16.0, 6.0);
    let knock = if c.tau >= 0.46 {
        (-(c.tau - 0.46) * 6.0).exp()
    } else {
        0.0
    };
    p.plant = 1.0 - raise;
    p.slide = -0.06;
    p.arm_r = rx(-1.05 - 0.25 * raise);
    p.squash *= 1.0 - 0.05 * knock;
    p.hat_lift = 0.03 * knock * thump.abs();
    p.swing = Vec2::new(0.22 * thump, 0.0);

    const EVERY: f32 = 4.6;
    let f = (c.tau / EVERY).fract() * EVERY;
    let shake = pulse(0.5, 0.7, 1.6, 1.9, f);
    let tap =
        pulse(2.4, 2.5, 3.5, 3.6, f) * (c.t * 9.0).sin().max(0.0);
    p.body = rx(-0.1);
    p.hips.z = -0.015;
    p.head = ry(0.36 * shake * (f * 10.5).sin())
        * rz(0.1 * (1.0 - shake))
        * rx(-0.12);
    p.lids *= 0.62;
    // The instruction, held out for a second look.
    p.arm_l = arm_l(0.3, 1.5 + 0.1 * shake * (f * 5.25).sin());
    p.prop = Prop::Warrant;
    p.prop_size = 1.0;
    p.leg_l = -0.2 * tap;
    p.lift_l = 0.012 * tap;
    // Two beats and a pause.
    let g = (c.t / 1.5).fract();
    let beat = arc(0.0, 0.16, g) + arc(0.24, 0.4, g);
    p.glow = signal(LIT * (0.6 + 0.45 * beat));
    p
}

pub(super) fn fault(c: &Context) -> Pose {
    let mut p = Pose::stand();
    let t = c.t;
    // Tics at odd moments.
    const EVERY: f32 = 0.42;
    let n = (t / EVERY).floor();
    let f = (t - n * EVERY) / EVERY;
    let tic = if dice(n as i32, c.q.seed) < 0.3 {
        arc(0.0, 0.5, f)
    } else {
        0.0
    };
    let side = if dice(n as i32 + 5, c.q.seed) < 0.5 {
        -1.0
    } else {
        1.0
    };
    // Reeling, as if dizzy.
    let (rs, rc) = (t * 2.1).sin_cos();
    p.body = rz(0.17 + 0.05 * rs) * rx(0.08 + 0.04 * rc);
    p.hips.x = -0.015 * rs;
    p.head = ry(0.25 * side * tic)
        * rz(0.42 + 0.08 * (t * 2.1 - 0.8).sin())
        * rx(0.2);
    p.head_lift = -0.015;
    p.hat = rz(-0.3) * rx(0.1);
    p.hat_lift = 0.012 + 0.02 * tic;
    p.lids = Vec2::new(1.0, 0.45);
    p.wide = 1.1;
    p.gaze =
        Vec2::new(0.16 * (t * 3.0).sin(), 0.012 * (t * 3.0).cos());
    p.arm_l = rz(-0.5) * rx(-0.3 - 0.5 * tic);
    p.arm_r = rz(0.7) * rx(-0.6);
    p.leg_l = -0.2;
    p.pole = rz(-0.85) * rx(0.3);
    p.swing =
        Vec2::new(0.3 * (t * 3.3).sin(), 0.22 * (t * 2.7).sin());
    let dropped = dice((t * 2.6).floor() as i32 + 11, c.q.seed) < 0.4;
    p.glow = inlay::fault() * if dropped { LIT * 0.08 } else { LIT };
    p.key = (t * 3.0).floor() * 0.7;
    p
}

pub(super) fn failed(c: &Context) -> Pose {
    let mut p = Pose::stand();
    let x = c.tau;
    let reel = (1.0 - ramp(0.0, 0.55, x)) * (x * 14.0).sin();
    let sit = ramp(0.45, 0.95, x).powf(1.6);
    let bump = ring(x - 0.95, 11.0, 6.0);
    let droop = ramp(1.0, 2.2, x);
    let fall = ((x - 0.7) / 0.55).clamp(0.0, 1.0).powi(2);
    p.hips.y = -0.2 * sit + 0.02 * bump;
    p.squash = 1.0 - 0.07 * bump;
    p.body = rz(0.09 * reel) * rx(0.14 * sit);
    p.leg_l = -1.42 * sit;
    p.leg_r = -1.36 * sit;
    p.head = rx(0.62 * droop) * rz(0.05 * reel);
    p.head_lift = -0.02 * droop;
    p.hat = rx(0.16 * droop);
    p.hat_lift = 0.03 * arc(0.95, 1.2, x);
    p.lids = Vec2::splat(1.0 - 0.94 * ramp(1.1, 2.0, x));
    p.arm_l = rz(-0.14) * rx(-0.35 * sit);
    p.arm_r =
        rx(-1.05).slerp(rz(0.14) * rx(-0.35), ramp(0.7, 1.2, x));
    // The crook is let go and topples.
    p.grip = 1.0 - ramp(0.6, 0.72, x);
    p.pole = rz(-0.06).slerp(ry(0.5) * rx(1.42), fall);
    p.pole_at = Vec3::new(0.3, POLE_FOOT, 0.26)
        .lerp(Vec3::new(0.42, 0.1, -0.1), fall);
    p.hang = 1.0 - fall;
    p.swing = Vec2::new(0.2 * reel, 0.0);
    let dying = 1.0 - ramp(0.2, 1.9, x);
    let gutter = 0.6 + 0.4 * (x * 31.0).sin();
    p.glow = inlay::fault() * (LIT * 0.6 * dying * gutter);
    p
}
