//! A robot in a chief's hand: poked, patted, held up and set down.

use bevy::prelude::*;
use hamlet_vfx::inlay::{EMBER, LIT};

use super::Ctx;
use super::parts::{arm_high, arm_l, arm_r, blink, flicker};
use crate::keeper::pose::{
    Pose, arc, candle, pulse, ramp, ring, rx, ry, rz,
};

pub(super) fn startle(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    let x = c.tau;
    // A hop of fright, then it finds who did it and waves.
    let jump = arc(0.0, 0.36, x);
    let land = ring(x - 0.36, 15.0, 7.0) * ramp(0.36, 0.38, x);
    let fright = pulse(0.0, 0.06, 0.5, 0.8, x);
    let up = pulse(0.55, 0.85, 2.0, 2.35, x);
    let wave = pulse(0.9, 1.1, 1.95, 2.2, x);
    p.shift.y = 0.16 * jump * c.q.bounce;
    p.squash = 1.0 + 0.08 * jump - 0.07 * land;
    p.hips.y = -0.02 * land;
    p.leg_l = -0.3 * jump;
    p.leg_r = 0.25 * jump;
    p.lift_l = 0.05 * jump;
    p.lift_r = 0.05 * jump;
    p.wide = 1.0 + 0.4 * fright + 0.1 * up;
    p.hat_lift = 0.1 * arc(0.02, 0.55, x) + 0.02 * wave;
    p.hat = rz(-0.2 * arc(0.02, 0.55, x));
    p.body = rx(-0.16 * fright - 0.08 * up);
    p.head = rx(-0.5 * up) * rz(0.1 * wave * (x * 6.5).sin());
    p.gaze.y = 0.012 * up;
    p.arm_l = (rz(-0.14 - 0.9 * fright) * rx(-0.5 * fright))
        .slerp(arm_high(2.45 + 0.32 * (x * 13.0).sin()), wave);
    p.arm_r = rx(-1.05 - 0.4 * fright);
    p.plant = 1.0 - fright;
    p.swing = Vec2::new(0.3 * ring(x - 0.05, 9.0, 3.0), 0.1 * land);
    // The eyes smile once it knows it is only you.
    p.lids *= 1.0 - 0.35 * wave;
    p.glow = candle(EMBER * 1.4 + LIT * (0.7 * fright + 0.4 * wave));
    p.key = 8.0 * ramp(0.0, 0.5, x);
    p
}

pub(super) fn sulk(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    blink(&mut p, c);
    let x = c.tau;
    // A stamp, then arms crossed and a huff, and a look back to see
    // whether it is being watched.
    let stamp = ring(x - 0.22, 17.0, 8.0) * ramp(0.2, 0.22, x);
    let raise = arc(0.0, 0.22, x);
    let cross = pulse(0.25, 0.6, 3.0, 3.5, x);
    let huff = arc(0.9, 1.25, x) + arc(1.9, 2.25, x);
    let peek = pulse(2.45, 2.7, 2.95, 3.2, x);
    let tap = (x * 8.0).sin().max(0.0).powi(2)
        * pulse(1.2, 1.4, 2.4, 2.6, x);
    p.lift_r = 0.05 * raise;
    p.leg_l = -0.18 * tap;
    p.lift_l = 0.012 * tap;
    p.hips.y = -0.025 * stamp;
    p.squash = 1.0 - 0.06 * stamp + 0.05 * huff;
    p.arm_l = p.arm_l.slerp(arm_l(1.25, 1.1), cross);
    p.arm_r = p.arm_r.slerp(arm_r(0.9, 1.3), cross);
    p.plant = 1.0 - cross;
    p.slide = 0.2;
    p.pole = p.pole.slerp(rz(0.5) * rx(0.2), cross);
    p.body = rx(0.1 * cross - 0.06 * huff);
    p.head = ry(1.0 * peek) * rz(0.14 * cross) * rx(0.22 * cross);
    p.gaze.x = 0.35 * peek;
    p.hat = rx(0.16 * cross);
    p.hat_lift = 0.03 * huff;
    p.lids *= 1.0 - 0.45 * cross * (1.0 - peek);
    p.swing = Vec2::new(0.12 * stamp, 0.0);
    p.glow = candle(EMBER * (1.4 - 0.5 * cross) + LIT * 0.5 * huff);
    p
}

pub(super) fn wiggle(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    let x = c.tau;
    let on = pulse(0.0, 0.2, 1.1, 1.45, x);
    let s = (x * 15.0).sin() * on;
    let bob = (x * 30.0).cos().abs() * on;
    p.hips.x = 0.035 * s * c.q.sway;
    p.hips.y = -0.012 * bob;
    p.body = rz(-0.16 * s) * ry(0.2 * s);
    p.squash = 1.0 + 0.03 * bob;
    p.head = rz(0.2 * s) * rx(-0.16 * on);
    p.hat_lift = 0.03 * bob;
    p.hat = rz(-0.12 * s);
    p.arm_l = p.arm_l.slerp(rz(-0.7) * rx(-0.5 - 0.3 * s), on);
    p.shift.y = 0.03 * arc(1.05, 1.4, x);
    // Eyes squeezed into a smile.
    p.lids *= 1.0 - 0.6 * on;
    p.swing = Vec2::new(0.0, 0.3 * (x * 15.0 - 1.2).sin() * on);
    p.glow = candle(EMBER * 1.4 + LIT * 0.6 * on);
    p.key = c.t * 9.0;
    p.flag = 0.5 * s;
    p
}

pub(super) fn dangle(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    blink(&mut p, c);
    let x = c.tau;
    // It kicks hard at first, then in bouts.
    let bout = 0.35
        + 0.65
            * pulse(0.0, 0.2, 0.6, 0.8, (x / 1.9).fract())
                .max(1.0 - ramp(0.8, 1.6, x));
    let kick = (x * 11.0).sin() * bout;
    let sway = (x * 2.3).sin();
    p.hips.y = 0.02;
    p.squash = 1.06;
    p.leg_l = 0.25 + 0.6 * kick;
    p.leg_r = 0.25 - 0.6 * kick;
    p.body = rz(0.1 * sway) * rx(0.1 + 0.05 * kick);
    p.head = ry(0.5 * (x * 1.3).sin()) * rx(0.3) * rz(-0.1 * sway);
    p.gaze.x = 0.2 * (x * 1.3 + 0.6).sin();
    p.wide = 1.2 + 0.1 * bout;
    p.hat_lift = 0.02 + 0.015 * kick.abs();
    p.hat = rz(0.12 * sway);
    p.arm_l = rz(-1.1 - 0.5 * kick.abs()) * rx(-0.4 * kick);
    p.arm_r = rx(-1.6 + 0.3 * kick);
    p.plant = 0.0;
    p.slide = 0.12;
    p.pole = rz(0.25 * sway) * rx(0.3 + 0.2 * kick);
    p.swing = Vec2::new(0.25 * kick, 0.4 * (x * 2.3 - 1.0).sin());
    p.glow = candle(EMBER * 1.4) * (1.0 + 0.2 * bout * flicker(c.t));
    p.key = c.t * 9.0;
    p.flag = 0.6 * sway;
    p
}

pub(super) fn land(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    let x = c.tau;
    let hit = ring(x, 15.0, 6.5);
    let shake = pulse(0.3, 0.42, 0.68, 0.85, x);
    p.hips.y = -0.05 * hit.max(0.0);
    p.squash = 1.0 - 0.16 * hit;
    p.shift.y = 0.05 * arc(0.14, 0.4, x);
    p.hat_lift = 0.09 * arc(0.03, 0.42, x);
    p.hat = rx(-0.2 * arc(0.03, 0.42, x));
    p.wide = 1.0 + 0.25 * (1.0 - ramp(0.1, 0.5, x));
    p.head = ry(0.35 * shake * (x * 21.0).sin()) * rx(0.12 * hit);
    p.arm_l = rz(-0.14 - 0.7 * (1.0 - ramp(0.0, 0.5, x)));
    p.plant = ramp(0.0, 0.3, x);
    p.swing = Vec2::new(0.35 * hit, 0.15 * ring(x, 9.0, 4.0));
    p.glow = candle(EMBER * 1.4 + LIT * 0.5 * hit.abs());
    p
}
