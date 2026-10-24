//! Two robots face to face: a letter passed, talk, a greeting.

use bevy::prelude::*;
use hamlet_vfx::inlay::{EMBER, LIT};

use super::Context;
use super::parts::{
    arm_high, arm_l, blink, breathe, crook_out, current, shift_weight,
};
use crate::keeper::pose::{
    Pose, Prop, arc, candle, pulse, ramp, rx, ry, rz,
};

/// The moment a letter changes hands, in seconds from the start of
/// either motion.
pub(super) const PASS: f32 = 0.95;

pub(super) fn hand_over(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    let x = c.tau;
    let wind = pulse(0.0, 0.28, 0.3, 0.55, x);
    let reach = ramp(0.32, 0.78, x) - ramp(1.15, 1.7, x);
    let done = ramp(1.15, 1.75, x);
    let thanks = pulse(1.25, 1.42, 1.45, 1.75, x);
    let hold = arm_l(0.35 - 0.2 * wind, 1.2 - 0.25 * wind);
    p.arm_l = hold
        .slerp(arm_l(0.8, 1.5), ramp(0.32, 0.78, x))
        .slerp(rz(-0.14), done);
    p.body = ry(0.35 * reach) * rx(0.2 * reach - 0.09 * wind);
    p.hips.z = 0.05 * reach - 0.025 * wind;
    p.hips.y = -0.012 * wind + 0.012 * thanks;
    p.squash *= 1.0 - 0.04 * wind;
    p.head = rz(0.14 * reach) * rx(-0.12 * reach + 0.22 * thanks);
    p.hat_lift = 0.012 * thanks;
    if x < PASS {
        p.prop = Prop::Letter;
        p.prop_size = 1.0;
    }
    crook_out(&mut p, 1.0);
    let after = ramp(PASS, 1.6, x);
    p.glow = current(LIT).lerp(candle(EMBER * 1.4), after);
    p.ring = 1.0 - after;
    p.key = c.t * 3.0;
    p
}

pub(super) fn receive(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    let x = c.tau;
    let out = ramp(0.45, 0.85, x);
    let reach = out - ramp(1.05, 1.6, x);
    let read = ramp(1.05, 1.65, x);
    let start = pulse(0.9, 1.0, 1.02, 1.25, x);
    p.arm_l = rz(-0.14)
        .slerp(arm_l(0.8, 1.5), out)
        .slerp(arm_l(0.55, 1.3), read);
    p.body = ry(0.35 * reach) * rx(0.2 * reach);
    p.hips.z = 0.05 * reach;
    p.hips.y = 0.015 * start;
    p.head = rz(-0.12 * reach + 0.1 * read)
        * rx(-0.12 * reach + 0.24 * read);
    p.wide = 1.0 + 0.25 * start;
    p.hat_lift = 0.02 * start;
    // The eyes run along the lines.
    p.gaze.x = 0.14 * read * (c.t * 4.0).sin();
    if x >= PASS {
        p.prop = Prop::Letter;
        p.prop_size = 1.0;
    }
    crook_out(&mut p, 1.0);
    let after = ramp(PASS, 1.5, x);
    p.glow = candle(EMBER * 1.4).lerp(current(LIT), after);
    p.key = c.t * 5.0 * after;
    p
}

/// The rise and fall of a voice.
pub(super) fn voice(t: f32) -> f32 {
    let syllable = 0.5 + 0.5 * (t * 11.0).sin();
    let phrase = (0.5 + 0.5 * (t * 2.1).sin()).powf(0.6);
    syllable * phrase
}

pub(super) fn talk(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    let v = voice(c.t);
    let beat = (c.t * 2.6).sin();
    let turn = (c.t * 1.3).sin();
    p.arm_l = arm_l(0.3 + 0.2 * turn, 0.95 + 0.35 * beat)
        * rz(-0.15 * beat);
    p.body = ry(0.07 * turn) * rx(0.09 + 0.02 * beat);
    p.hips.y = 0.008 * (c.t * 5.2).sin();
    p.head = rz(0.09 * turn) * rx(0.07 * (c.t * 5.2 - 0.6).sin());
    p.hat_lift = 0.005 * v;
    p.swing = Vec2::new(0.05 * (c.t * 2.6 - 1.0).sin(), 0.0);
    crook_out(&mut p, 1.0);
    p.glow = current(LIT * (0.5 + 0.6 * v));
    p.key = c.t * 5.0;
    p
}

pub(super) fn listen(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    shift_weight(&mut p, c);
    let nod = (c.t * 2.8).sin().max(0.0).powi(2);
    p.body *= rx(0.06);
    p.head = p.head * rz(0.17) * rx(-0.06 + 0.11 * nod);
    crook_out(&mut p, 1.0);
    p.glow = current(LIT * 0.6);
    p.key = c.t * 2.0;
    p
}

pub(super) fn greet(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    let x = c.tau;
    let wave = pulse(0.1, 0.3, 0.85, 1.05, x);
    let reach = pulse(0.95, 1.2, 1.6, 1.85, x);
    let tip = pulse(1.15, 1.3, 1.5, 1.7, x);
    let bow = pulse(1.1, 1.4, 1.55, 1.85, x);
    p.arm_l = p
        .arm_l
        .slerp(arm_high(2.4 + 0.3 * (x * 13.0).sin()), wave)
        .slerp(arm_l(0.55, 2.45), reach);
    p.shift.y = 0.03 * arc(0.15, 0.5, x);
    p.hat_lift = 0.07 * tip;
    p.hat = rx(0.35 * tip);
    p.body = rx(0.32 * bow - 0.05 * wave);
    p.hips.z = -0.03 * bow;
    p.head = rz(0.12 * wave) * rx(0.2 * bow - 0.1 * wave);
    p.lids *= 1.0 - 0.4 * wave.max(tip);
    crook_out(&mut p, pulse(0.0, 0.3, 1.6, 1.95, x));
    p.glow = candle(EMBER * 1.4 + LIT * 0.5 * wave.max(bow));
    p.key = c.t * 4.0;
    p
}

pub(super) fn wave(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    let x = c.tau;
    let up = pulse(0.05, 0.3, 1.15, 1.5, x);
    p.arm_l =
        p.arm_l.slerp(arm_high(2.45 + 0.32 * (x * 12.0).sin()), up);
    p.shift.y = 0.025 * arc(0.2, 0.7, x);
    p.body = rz(0.05 * up);
    p.head = rz(0.14 * up);
    p.hat_lift = 0.015 * arc(0.3, 0.8, x);
    p.lids *= 1.0 - 0.3 * up;
    p.glow = candle(EMBER * 1.4 + LIT * 0.3 * up);
    p
}
