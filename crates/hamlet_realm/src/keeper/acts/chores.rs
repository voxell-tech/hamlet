//! A robot at work about its own house, and the end of its work.

use std::f32::consts::TAU;

use bevy::prelude::*;
use hamlet_vfx::inlay::{self, EMBER, LIT};

use super::parts::{
    arm_high, arm_l, arm_r, blink, breathe, current, moon,
};
use super::{CRANK_RATE, Context, SEAT_BACK};
use crate::keeper::kit::KITE;
use crate::keeper::pose::{
    POLE_FOOT, Pose, Prop, arc, back_out, candle, dice, pulse, ramp,
    ring, rx, ry, rz,
};
use crate::keeper::work::RING_EVERY;

pub(super) fn think(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    // A toe taps for a while, then rests.
    let bout = pulse(0.0, 0.1, 0.45, 0.5, (c.t / 4.6).fract());
    let tap = (c.t * 9.0).sin().max(0.0).powi(2) * bout;
    // Now and then it has an idea.
    let idea = arc(0.0, 0.07, (c.t / 6.2).fract());
    p.arm_l = arm_l(0.62, 2.3);
    p.body = rz(0.03 * (c.t * 0.8).sin()) * rx(-0.03);
    p.hips.x = 0.01 * (c.t * 0.8).sin();
    p.hips.y = 0.03 * idea;
    p.head = ry(0.22 * (c.t * 0.5).sin()) * rz(-0.15) * rx(-0.17);
    p.gaze = Vec2::new(0.2 * (c.t * 0.7).sin(), 0.012);
    p.wide = 1.0 + 0.25 * idea;
    p.hat_lift = 0.03 * idea;
    p.leg_r = -0.2 * tap;
    p.lift_r = 0.012 * tap;
    p.swing = Vec2::new(0.0, 0.04 * (c.t * 0.8 - 1.0).sin());
    p.glow =
        current(LIT * (0.7 + 0.3 * (c.t * 2.4).sin() + 0.5 * idea));
    p.key = c.t * 5.0;
    p
}

/// The crook stuck in the ground beside it, leaving both hands free.
pub(super) fn set_crook_aside(p: &mut Pose) {
    p.grip = 0.0;
    p.pole = rz(-0.05) * rx(-0.04);
    p.pole_at = Vec3::new(0.46, POLE_FOOT - 0.03, -0.08);
}

/// `bench` raises the blow to the height of a bench top.
pub(super) fn hammer(c: &Context, bench: bool) -> Pose {
    let mut p = Pose::stand();
    blink(&mut p, c);
    set_crook_aside(&mut p);
    let u = (c.t / 0.95).fract();
    // A slow lift, a fast blow, and a rest on the work.
    let (top, hit, rest) = if bench {
        (2.9, 1.5, 1.62)
    } else {
        (2.7, 0.8, 0.95)
    };
    let (lift, angle) = if u < 0.55 {
        let k = ramp(0.0, 0.55, u);
        (k, rest + (top - rest) * k)
    } else if u < 0.68 {
        let k = ((u - 0.55) / 0.13).powi(2);
        (1.0 - k, top + (hit - top) * k)
    } else {
        (0.0, hit + (rest - hit) * ramp(0.68, 1.0, u))
    };
    let blow = if u >= 0.68 {
        (-(u - 0.68) * 9.0).exp()
    } else {
        0.0
    };
    p.arm_l = rz(-0.08) * rx(-angle);
    p.arm_r = arm_r(0.5, 0.75);
    p.body = ry(0.12 * lift) * rx(0.2 - 0.22 * lift + 0.1 * blow);
    p.hips.y = -0.03 + 0.03 * lift - 0.015 * blow;
    p.squash = 1.0 + 0.03 * lift - 0.07 * blow;
    p.head = rx(0.16 - 0.12 * lift + 0.12 * blow);
    p.hat_lift = 0.035 * arc(0.68, 0.9, u);
    p.lids *= 1.0 - 0.9 * arc(0.66, 0.8, u);
    p.swing = Vec2::new(0.16 * blow * ((u - 0.68) * 40.0).cos(), 0.0);
    p.prop = Prop::Mallet;
    p.prop_size = 1.0;
    p.glow = current(LIT * (0.7 + 0.6 * blow));
    p.key = c.t * 5.0;
    p
}

pub(super) fn lift(c: &Context) -> Pose {
    let mut p = Pose::stand();
    blink(&mut p, c);
    set_crook_aside(&mut p);
    let u = (c.t / 4.2).fract();
    let squat = pulse(0.0, 0.15, 0.28, 0.42, u);
    let strain = pulse(0.15, 0.19, 0.27, 0.31, u);
    let heave = pulse(0.28, 0.36, 0.4, 0.5, u);
    let up = ramp(0.28, 0.45, u) - ramp(0.86, 0.97, u);
    let place = pulse(0.68, 0.8, 0.85, 0.95, u);
    let carry = up * (1.0 - place);
    let reach = 0.55 + 0.85 * up + 0.3 * place;
    p.arm_l = arm_l(0.42, reach);
    p.arm_r = arm_r(0.42, reach);
    p.hips.y = -0.11 * squat + 0.02 * place;
    p.hips.x = 0.004 * strain * (c.t * 60.0).sin();
    p.hips.z = 0.03 * place - 0.02 * carry;
    p.body = rz(0.03 * carry * (c.t * 3.0).sin())
        * rx(0.45 * squat - 0.15 * carry + 0.2 * place);
    p.squash = 1.0 - 0.06 * squat + 0.05 * heave;
    p.head = rx(0.3 * squat - 0.1 * carry);
    p.hat_lift = 0.02 * heave;
    p.lids *= 1.0 - 0.6 * strain;
    p.prop = Prop::Block;
    p.prop_size = ramp(0.01, 0.1, u) * (1.0 - ramp(0.87, 0.94, u));
    p.glow = current(LIT * (0.75 + 0.35 * strain + 0.2 * heave));
    p.key = c.t * 5.0;
    p
}

pub(super) fn celebrate(c: &Context) -> Pose {
    let mut p = Pose::stand();
    blink(&mut p, c);
    let x = c.tau;
    let mut air = 0.0;
    let mut crouch = 0.0;
    let mut land = 0.0;
    let mut hat = 0.0;
    for start in [0.1, 1.1] {
        let y = x - start;
        crouch += pulse(0.0, 0.14, 0.16, 0.22, y);
        air += arc(0.18, 0.62, y);
        hat += arc(0.3, 0.8, y);
        land += ring(y - 0.62, 15.0, 7.0) * ramp(0.62, 0.64, y);
    }
    let up = ramp(0.1, 0.35, x) - ramp(1.9, 2.3, x);
    p.shift.y = 0.24 * air * c.q.bounce;
    p.spin = ry(TAU * ramp(1.28, 1.72, x));
    p.hips.y = -0.06 * crouch - 0.03 * land;
    p.squash = 1.0 - 0.12 * crouch + 0.1 * air - 0.1 * land;
    p.lift_l = 0.06 * air;
    p.lift_r = 0.04 * air;
    p.leg_l = -0.3 * air;
    p.leg_r = 0.25 * air;
    p.arm_l =
        p.arm_l.slerp(arm_high(2.4 + 0.25 * (x * 9.0).sin()), up);
    p.arm_r = rx(-1.05 - 0.85 * up);
    p.plant = 1.0 - up;
    p.slide = 0.03;
    p.pole = rz(-0.06 * (1.0 - up) + 0.12 * up * (x * 9.0).sin());
    p.head = rx(-0.22 * up);
    p.hat_lift = 0.07 * hat;
    // Eyes squeezed into a smile.
    p.lids *= 1.0 - 0.5 * up;
    p.swing = Vec2::new(0.1 * land, 0.3 * up * (x * 9.0 - 1.2).sin());
    p.glow = candle(EMBER * 1.4 + LIT * (0.5 * up + 0.5 * air));
    p.key = c.t * 6.0;
    p
}

pub(super) fn vote(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    let x = c.tau;
    let hold = ramp(0.0, 0.4, x);
    let weigh = pulse(0.3, 0.5, 1.2, 1.4, x);
    let nod = arc(1.3, 1.6, x);
    let cast = ramp(1.5, 1.95, x) - ramp(2.75, 3.2, x);
    let down = ramp(2.75, 3.25, x);
    p.arm_l = rz(-0.14)
        .slerp(arm_l(0.5, 1.3), hold)
        .slerp(arm_high(2.75), ramp(1.5, 1.95, x))
        .slerp(rz(-0.14), down);
    p.head = rz(0.14 * weigh * (x * 4.4).sin())
        * rx(0.22 * hold * (1.0 - cast) + 0.2 * nod - 0.18 * cast);
    p.gaze.x = 0.1 * weigh * (x * 4.4 + 1.0).sin();
    p.shift.y = 0.025 * cast;
    p.squash *= 1.0 - 0.05 * arc(1.4, 1.6, x) + 0.04 * cast;
    p.hat_lift = 0.02 * arc(1.9, 2.2, x);
    p.prop = Prop::Ballot;
    p.prop_size = hold * (1.0 - ramp(2.9, 3.1, x));
    // It weighs the choice by its own notes, then shows it.
    p.glow = moon(LIT * 0.6)
        .lerp(candle(LIT * 0.9), ramp(1.4, 1.9, x))
        .lerp(candle(EMBER * 1.4), down);
    p
}

/// On its seat, legs out in front.
pub(super) fn sit(p: &mut Pose) {
    p.shift.z = -SEAT_BACK;
    p.hips.y = -0.11;
    p.leg_l = -1.32;
    p.leg_r = -1.38;
}

/// `jot` has it write in the book it would otherwise read.
pub(super) fn read(c: &Context, jot: bool) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    set_crook_aside(&mut p);
    sit(&mut p);
    const EVERY: f32 = 4.4;
    let n = (c.t / EVERY).floor();
    let f = c.t - n * EVERY;
    // Most spans start with a turned page. Now and then it looks up
    // and thinks about what it read.
    let muses = dice(n as i32 + 211, c.q.seed) < 0.24;
    let (turn, up) = if muses {
        (0.0, pulse(0.3, 0.9, 2.6, 3.3, f))
    } else {
        (pulse(0.0, 0.18, 0.3, 0.55, f), 0.0)
    };
    p.arm_l = arm_l(0.42, 1.3);
    p.body = rx(0.1 - 0.12 * up);
    p.head = ry(-0.12 + 0.34 * up) * rx(0.34 - 0.62 * up);
    p.gaze.x = 0.14 * (1.0 - up) * (c.t * 3.6).sin();
    p.prop = Prop::Book;
    p.prop_size = 1.0;
    p.key = c.t * 2.0;
    if jot {
        let pen = (c.t * 17.0).sin() * (1.0 - up);
        p.arm_r = arm_r(
            0.62 + 0.12 * (c.t * 2.3).sin() * (1.0 - up),
            1.2 + 0.04 * pen,
        );
        p.glow = current(LIT * (0.55 + 0.3 * pen.abs() + 0.3 * up));
    } else {
        p.arm_r = arm_r(0.3, 0.55).slerp(arm_r(0.75, 1.25), turn);
        p.glow = current(LIT * (0.55 + 0.25 * turn + 0.3 * up));
    }
    p
}

pub(super) fn write(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 0.8);
    blink(&mut p, c);
    set_crook_aside(&mut p);
    // A line is written across, then the hand comes back. Every few
    // lines it dips the quill, or stops to read the page over.
    const LINE: f32 = 1.9;
    let n = (c.t / LINE).floor();
    let f = (c.t - n * LINE) / LINE;
    let pick = dice(n as i32 + 419, c.q.seed);
    let (dips, rereads) = (pick < 0.2, (0.2..0.36).contains(&pick));
    let across = ramp(0.0, 0.78, f) - ramp(0.82, 1.0, f);
    let writing = if dips || rereads {
        0.0
    } else {
        pulse(0.0, 0.06, 0.76, 0.82, f)
    };
    let dip = if dips {
        pulse(0.1, 0.4, 0.5, 0.85, f)
    } else {
        0.0
    };
    let reread = if rereads {
        pulse(0.05, 0.3, 0.7, 0.95, f)
    } else {
        0.0
    };
    let scratch = writing * (c.t * 23.0).sin();
    let reach = if dips || rereads {
        0.3
    } else {
        0.08 + 0.5 * across
    };
    p.arm_l = arm_l(reach, 1.5 + 0.035 * scratch)
        .slerp(arm_l(0.95, 1.72), dip)
        .slerp(arm_l(0.2, 1.1), reread);
    p.arm_r = arm_r(0.3, 1.2);
    p.body = rx(0.22 - 0.1 * reread) * ry(0.05 * across);
    p.hips.z = -0.02;
    p.head = ry(0.25 * (reach - 0.3) + 0.3 * dip)
        * rz(0.1 * reread)
        * rx(0.3 - 0.1 * reread);
    p.gaze.x = 0.12 * reread * (c.t * 4.0).sin();
    p.hat_lift = 0.004 * scratch.abs();
    p.prop = Prop::Quill;
    p.prop_size = 1.0;
    p.glow = current(
        LIT * (0.5 + 0.45 * writing * scratch.abs() + 0.2 * reread),
    );
    p.key = c.t * 4.0;
    p
}

/// A stroke of the hand on the rim of the wheel, with the wheel
/// `turn` radians round.
pub(super) fn crank_into(p: &mut Pose, c: &Context, turn: f32) {
    let s = turn.sin();
    let heave = (-s).max(0.0);
    p.arm_l = arm_l(0.12, 1.5 + 0.55 * s);
    p.arm_r = arm_r(0.35, 0.85);
    p.body = ry(0.1 * s) * rx(0.2 - 0.1 * s);
    p.hips.y = -0.02 + 0.018 * s;
    p.squash = 1.0 + 0.025 * s;
    p.head = rx(0.12 - 0.1 * s);
    p.hat_lift = 0.012 * heave;
    p.swing = Vec2::new(0.1 * (turn - 0.8).sin(), 0.0);
    p.glow = current(LIT * (0.7 + 0.4 * heave));
    p.key = c.t * 6.0;
}

pub(super) fn crank(c: &Context) -> Pose {
    let mut p = Pose::stand();
    blink(&mut p, c);
    set_crook_aside(&mut p);
    crank_into(&mut p, c, c.tau * CRANK_RATE);
    p
}

pub(super) fn tend(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    set_crook_aside(&mut p);
    let x = c.tau;
    // It steps back and watches each ring of steam go up.
    let rise = ramp(0.05, 0.8, (x / RING_EVERY).fract());
    let bout = pulse(0.0, 0.1, 0.4, 0.45, (c.t / 5.3).fract());
    let tap = (c.t * 9.0).sin().max(0.0).powi(2) * bout;
    p.shift.z = -0.1;
    p.spin = ry(-0.45);
    p.arm_l = rz(-0.5) * rx(-0.25);
    p.arm_r = rz(0.5) * rx(-0.25);
    p.body = rx(-0.05);
    p.head = ry(0.4) * rx(0.06 - 0.5 * rise);
    p.gaze.y = 0.01 * rise;
    p.leg_r = -0.2 * tap;
    p.lift_r = 0.012 * tap;
    p.glow = current(LIT * (0.55 + 0.15 * (c.t * 1.2).sin()));
    p.key = c.t * 2.5;
    // After a long run the head begins to sink.
    let weary = ramp(24.0, 28.0, x);
    let f = (x / 7.0).fract();
    let nod = weary * (ramp(0.0, 0.86, f) - ramp(0.86, 0.93, f));
    p.head *= rx(0.5 * nod);
    p.lids *= 1.0 - 0.85 * nod;
    p.hat = rx(0.1 * nod);
    // Every so often it gives the wheel a few turns more.
    let helps = if x > 8.0 {
        pulse(0.0, 0.4, 1.7, 2.1, x % 9.0)
    } else {
        0.0
    };
    if helps > 0.0 {
        let mut q = Pose::stand();
        set_crook_aside(&mut q);
        crank_into(&mut q, c, x * CRANK_RATE);
        p = p.mix(&q, helps);
    }
    p
}

pub(super) fn kite(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    let x = c.tau;
    let up = ramp(0.0, 0.9, x);
    // A tug on the string, and a bead of light runs down it to the
    // hand.
    const EVERY: f32 = 3.4;
    let f = if x > 1.2 { (x - 1.2) % EVERY } else { EVERY };
    let tug = pulse(0.0, 0.12, 0.2, 0.5, f);
    let caught = arc(1.55, 2.1, f);
    let sway = 0.1 * (c.t * 0.9).sin() + 0.04 * (c.t * 2.3).sin();
    p.arm_l = rz(-0.14).slerp(
        arm_high(2.72 + sway - 0.1 * tug)
            * rx(0.1 * (c.t * 0.63).sin()),
        up,
    );
    p.body = rz(0.05 * up) * rx(-0.06 * up);
    p.head = ry(-0.5 * up) * rx(-0.42 * up);
    p.gaze.y = 0.01 * up;
    p.shift.y = 0.025 * caught;
    p.hat_lift = 0.02 * caught;
    p.wide = 1.0 + 0.15 * caught;
    p.prop = Prop::Kite;
    // The kite goes up once the arm is.
    p.prop_size = back_out((x - 0.4) / 0.9).max(0.0);
    p.bead = (KITE - 0.3)
        * (1.0 - ramp(0.25, 1.65, f))
        * ramp(0.0, 0.9, x);
    p.swing = Vec2::new(0.0, 0.06 * (c.t * 0.9 - 1.0).sin());
    p.glow = current(LIT * (0.6 + 0.6 * caught));
    p.key = c.t * 3.0;
    p
}

pub(super) fn recall(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    // The eyes go down the scroll, then a nod.
    let f = c.t % 5.0;
    let down = ramp(0.0, 3.4, f) - ramp(4.3, 5.0, f);
    let nod = arc(3.5, 4.2, f);
    p.arm_l = arm_high(2.2 + 0.03 * (c.t * 1.1).sin()) * rx(-0.2);
    p.body = rz(0.04);
    p.head = ry(-0.55) * rx(-0.22 + 0.36 * down + 0.16 * nod);
    p.gaze.x = 0.09 * (c.t * 5.0).sin();
    p.hat_lift = 0.01 * nod;
    p.prop = Prop::Scroll;
    p.prop_size = 1.0;
    p.glow = current(LIT * (0.6 + 0.4 * nod));
    p.key = c.t * 3.0;
    p
}

pub(super) fn stow(c: &Context) -> Pose {
    let mut p = Pose::stand();
    blink(&mut p, c);
    set_crook_aside(&mut p);
    let x = c.tau;
    // It rolls the scroll up, lays it on the desk and stamps it.
    let roll = ramp(0.6, 1.25, x);
    let lay = ramp(1.05, 1.55, x);
    let raise = pulse(1.45, 1.9, 1.97, 2.05, x);
    let blow = if x >= 2.05 {
        (-(x - 2.05) * 7.0).exp()
    } else {
        0.0
    };
    let flash = (-((x - 2.1) / 0.22).powi(2)).exp();
    let cheer = pulse(2.45, 2.7, 3.0, 3.35, x);
    p.arm_l = (arm_high(2.2) * rx(-0.2))
        .slerp(arm_l(0.3, 1.45), lay)
        .slerp(arm_high(2.4), cheer);
    p.arm_r = arm_r(0.4, 0.9 + 1.5 * raise);
    p.body = rx(0.22 * lay - 0.1 * raise + 0.1 * blow - 0.2 * cheer);
    p.head = ry(-0.5 * (1.0 - lay)) * rx(0.25 * lay - 0.3 * cheer);
    p.squash = 1.0 + 0.03 * raise - 0.07 * blow;
    p.hips.y = -0.015 * blow;
    p.shift.y = 0.05 * arc(2.5, 2.9, x);
    p.hat_lift = 0.04 * arc(2.05, 2.4, x) + 0.03 * arc(2.6, 3.0, x);
    p.lids *= 1.0 - 0.9 * arc(2.0, 2.2, x);
    p.prop = Prop::Scroll;
    p.prop_size = (1.0 - 0.5 * roll) * (1.0 - ramp(2.2, 2.45, x));
    p.glow = current(LIT * (0.7 + 1.3 * flash + 0.3 * cheer));
    p.key = c.t * 5.0;
    p
}

pub(super) fn oops(c: &Context) -> Pose {
    let mut p = Pose::stand();
    let x = c.tau;
    let back = ramp(0.0, 0.18, x) - ramp(1.4, 1.9, x);
    let fling = pulse(0.0, 0.1, 0.35, 0.6, x);
    let shake = pulse(0.5, 0.65, 1.15, 1.35, x);
    let scratch = pulse(1.0, 1.25, 1.6, 1.85, x);
    p.shift = Vec3::new(0.0, 0.1 * arc(0.0, 0.32, x), -0.1 * back);
    p.squash = 1.0 + 0.06 * arc(0.0, 0.3, x)
        - 0.06 * ring(x - 0.32, 14.0, 7.0) * ramp(0.32, 0.34, x);
    p.body = rx(-0.14 * back);
    p.wide = 1.0 + 0.35 * pulse(0.0, 0.05, 0.5, 0.8, x);
    p.hat_lift = 0.09 * arc(0.02, 0.5, x);
    p.hat = rz(0.25 * arc(0.02, 0.5, x));
    p.arm_l = (rz(-0.14 - fling) * rx(-0.4 * fling))
        .slerp(arm_l(0.62, 2.5), scratch);
    p.arm_r = rx(-1.05 - 0.45 * fling);
    p.plant = 1.0 - fling;
    p.head = ry(0.4 * shake * (x * 14.0).sin())
        * rz(-0.16 * scratch)
        * rx(-0.1 * back + 0.14 * scratch);
    p.swing = Vec2::new(0.25 * ring(x - 0.1, 9.0, 3.0), 0.0);
    let stutter = if x < 0.5 {
        0.6 + 0.4 * (x * 37.0).sin().signum()
    } else {
        1.0
    };
    p.glow = current(LIT * 0.8).lerp(
        inlay::fault() * (LIT * 1.25 * stutter),
        1.0 - ramp(0.9, 1.6, x),
    );
    p.key = (x * 4.0).floor() * 0.6;
    p
}

pub(super) fn done(c: &Context) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    let x = c.tau;
    let reach = pulse(0.1, 0.4, 1.1, 1.45, x);
    let tip = pulse(0.38, 0.58, 1.0, 1.2, x);
    let bow = pulse(0.45, 0.8, 1.05, 1.4, x);
    p.arm_l = p.arm_l.slerp(arm_l(0.55, 2.45), reach);
    p.hat_lift = 0.07 * tip + 0.03 * arc(1.55, 1.95, x);
    p.hat = rx(0.35 * tip);
    p.body = rx(0.3 * bow);
    p.hips.z = -0.03 * bow;
    p.head = rx(0.2 * bow);
    p.shift.y = 0.06 * arc(1.45, 1.85, x);
    p.lids *= 1.0 - 0.5 * tip;
    p.glow = current(LIT * 0.8)
        .lerp(candle(EMBER * 1.4), ramp(0.6, 1.6, x));
    p.key = c.t * 5.0 * (1.0 - ramp(0.6, 1.6, x));
    p
}
