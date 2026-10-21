//! The small pieces the motions are made of: a breath, a blink, a
//! look about, a hand held out.

use glam::{Quat, Vec2};

use super::Ctx;
use crate::keeper::pose::{Pose, arc, dice, ramp, rx, ry, rz};
pub(super) use crate::keeper::pose::{current, signal};

/// The arm reaching forward by `up` and across the chest by `inward`.
pub(super) fn arm_l(inward: f32, up: f32) -> Quat {
    ry(inward) * rx(-up)
}

pub(super) fn arm_r(inward: f32, up: f32) -> Quat {
    ry(-inward) * rx(-up)
}

/// The free arm raised out to the side by `up`, a paper in its hand
/// facing whoever is in front.
pub(super) fn arm_high(up: f32) -> Quat {
    rz(-up)
}

/// The crook held out to the side, clear of whoever is in front.
pub(super) fn crook_out(p: &mut Pose, by: f32) {
    p.arm_r = ry(0.95 * by) * p.arm_r;
    p.pole = ry(0.95 * by) * p.pole;
}

pub(super) fn breathe(p: &mut Pose, c: &Ctx, depth: f32) {
    let b = (c.t * 1.6).sin();
    p.squash *= 1.0 + 0.014 * depth * b;
    p.head_lift += 0.004 * depth * (c.t * 1.6 - 0.7).sin();
    p.arm_l = rz(-0.02 * depth * b) * p.arm_l;
    p.flag += 0.3 * (c.t * 2.3).sin() + 0.12 * (c.t * 6.1).sin();
}

pub(super) fn blink(p: &mut Pose, c: &Ctx) {
    const EVERY: f32 = 3.4;
    let n = (c.t / EVERY).floor();
    let f = c.t - n * EVERY;
    let n = n as i32;
    let start = 0.2 + dice(n, c.q.seed) * (EVERY - 0.9);
    let twice = dice(n + 7919, c.q.seed) > 0.75;
    let mut shut = arc(start, start + 0.16, f);
    if twice {
        shut = shut.max(arc(start + 0.24, start + 0.4, f));
    }
    p.lids *= 1.0 - shut;
}

/// Turns the head now and then, the eyes going first.
pub(super) fn look_about(p: &mut Pose, c: &Ctx, amount: f32) {
    const EVERY: f32 = 4.3;
    let n = (c.t / EVERY).floor();
    let f = c.t - n * EVERY;
    let n = n as i32;
    let aim = |n: i32| {
        let r = dice(n + 101, c.q.seed);
        if r < 0.4 {
            Vec2::ZERO
        } else if r < 0.6 {
            Vec2::new(-0.5, 0.03)
        } else if r < 0.8 {
            Vec2::new(0.5, 0.0)
        } else if r < 0.9 {
            // Up at its own lantern.
            Vec2::new(0.3, -0.28)
        } else {
            Vec2::new(-0.2, 0.2)
        }
    };
    let (was, now) = (aim(n - 1), aim(n));
    let head = was.lerp(now, ramp(0.1, 0.7, f)) * amount;
    let eyes = was.lerp(now, ramp(0.0, 0.22, f)) * amount;
    p.head = p.head * ry(head.x) * rx(head.y);
    p.gaze.x += (eyes.x - head.x) * 0.45;
    p.gaze.y -= (eyes.y - head.y) * 0.03;
}

/// Leans on one foot, then the other.
pub(super) fn shift_weight(p: &mut Pose, c: &Ctx) {
    const EVERY: f32 = 7.3;
    let n = (c.t / EVERY).floor();
    let f = c.t - n * EVERY;
    let side = |n: i32| {
        if dice(n + 313, c.q.seed) < 0.5 {
            -1.0
        } else {
            1.0
        }
    };
    let was = side(n as i32 - 1);
    let now = side(n as i32);
    let s = was + (now - was) * ramp(0.0, 1.1, f);
    p.hips.x += 0.012 * s * c.q.sway;
    p.body = rz(-0.02 * s * c.q.sway) * p.body;
    p.head = rz(0.012 * s) * p.head;
}

pub(super) fn flicker(t: f32) -> f32 {
    1.0 + 0.05 * (t * 7.1).sin() * (t * 2.3).sin()
}
