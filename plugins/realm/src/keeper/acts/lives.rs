//! A robot's first moments and its last, and one robot going over to
//! another.

use glam::{Vec2, Vec3};

use crate::keeper::acts::parts::{
    arm_high, arm_l, blink, breathe, crook_out,
};
use crate::keeper::acts::{Ctx, MERGE_GAP};
use crate::keeper::pose::{
    EMBER, LIT, Pose, arc, back_out, candle, pulse, ramp, ring, rx,
    ry, rz,
};

/// Folded up like a toy in its tin, by `k`.
pub(super) fn tuck(p: &mut Pose, k: f32) {
    p.hips.y -= 0.2 * k;
    p.head_lift -= 0.1 * k;
    p.squash *= 1.0 - 0.14 * k;
    p.body *= rx(0.1 * k);
    p.arm_l = p.arm_l.slerp(rz(0.02) * rx(-0.15), k.clamp(0.0, 1.0));
}

pub(super) fn appear(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, ramp(1.5, 2.5, c.tau));
    let x = c.tau;
    let unfold = back_out((x - 0.3) / 0.6);
    tuck(&mut p, 1.0 - unfold);
    p.size = Vec3::splat(back_out(x / 0.4).max(0.0));
    p.spin = ry(-1.4 * (1.0 - ramp(0.0, 0.9, x)));
    p.hat_lift += 0.06 * arc(0.75, 1.1, x);

    // The eyes open, then the lamps catch.
    let open = ramp(1.0, 1.12, x);
    let twice = arc(1.3, 1.44, x).max(arc(1.52, 1.66, x));
    p.lids = Vec2::splat((0.06 + 0.94 * open) * (1.0 - twice));
    let catching = if x < 1.45 {
        0.55 + 0.45 * (x * 40.0).sin().signum()
    } else {
        1.0
    };
    p.glow = candle(EMBER * 1.4 * ramp(1.1, 1.2, x) * catching);

    let look = -0.55 * pulse(1.6, 1.8, 2.0, 2.2, x)
        + 0.55 * pulse(2.05, 2.25, 2.4, 2.6, x);
    p.head = ry(look) * rx(-0.1 * ramp(2.5, 2.8, x));
    p.gaze.x = 0.3 * look;
    p.wide = 1.0 + 0.2 * pulse(1.1, 1.2, 2.5, 2.8, x);

    // A wave and a hop, to whoever is watching.
    let wave = pulse(2.3, 2.5, 2.95, 3.15, x);
    p.arm_l =
        p.arm_l.slerp(arm_high(2.3 + 0.3 * (x * 15.0).sin()), wave);
    p.shift.y = 0.07 * arc(2.45, 2.8, x);
    p.hat_lift += 0.03 * arc(2.55, 2.95, x);
    p.key = 9.0 * ramp(0.0, 1.6, x);
    p
}

pub(super) fn leave(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    let x = c.tau;
    breathe(&mut p, c, 1.0 - ramp(1.5, 2.2, x));
    // The hat comes off and it bows.
    let reach = pulse(0.1, 0.45, 1.25, 1.6, x);
    let tip = pulse(0.42, 0.62, 1.15, 1.35, x);
    let bow = pulse(0.5, 0.9, 1.2, 1.6, x);
    p.arm_l = p.arm_l.slerp(arm_l(0.55, 2.45), reach);
    p.hat_lift = 0.07 * tip;
    p.hat = rx(0.35 * tip);
    p.body = rx(0.5 * bow);
    p.hips.z = -0.04 * bow;
    p.head = rx(0.25 * bow);
    // Then it folds itself away, the light last.
    let fold = ramp(2.0, 2.8, x);
    tuck(&mut p, fold);
    p.lids = Vec2::splat(1.0 - 0.9 * bow.max(ramp(1.9, 2.4, x)));
    let shrink = ramp(2.8, 3.55, x).powi(2);
    p.size = Vec3::splat(1.0 - shrink);
    let dim = 1.0 - 0.7 * ramp(1.5, 2.3, x);
    p.glow = candle(EMBER * 1.4 * dim * (1.0 - ramp(3.0, 3.5, x)));
    p.key = -2.0 * ramp(0.0, 2.8, x);
    p
}

pub(super) fn merge_into(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    blink(&mut p, c);
    let x = c.tau;
    let go = ramp(0.1, 0.9, x);
    let steps = pulse(0.1, 0.25, 0.75, 0.9, x) * (x * 13.0).sin();
    let join = ramp(1.05, 1.7, x).powi(2);
    p.shift = Vec3::new(
        0.0,
        0.3 * join,
        MERGE_GAP * (0.3 * go + 0.7 * join),
    );
    p.spin = ry(5.0 * join);
    p.size = Vec3::splat(1.0 - join);
    p.leg_l = -0.4 * steps;
    p.leg_r = 0.4 * steps;
    p.hips.y = -0.012 * steps.abs();
    p.body = rx(0.16 * go);
    p.arm_l = rz(-0.14).slerp(rz(-1.0) * rx(-0.7), go);
    p.head = rx(-0.1 * go);
    p.wide = 1.0 + 0.15 * go;
    p.swing = Vec2::new(-0.2 * steps, 0.0);
    crook_out(&mut p, go);
    p.glow = candle(EMBER * 1.4 + LIT * (0.6 * go + 0.6 * join));
    p.key = c.t * 5.0;
    p
}

pub(super) fn merge_host(c: &Ctx) -> Pose {
    let mut p = Pose::stand();
    breathe(&mut p, c, 1.0);
    blink(&mut p, c);
    let x = c.tau;
    let open = pulse(0.2, 0.8, 1.6, 2.0, x);
    // It swells as the other goes in, then springs tall.
    let gulp = pulse(1.15, 1.6, 1.62, 1.75, x);
    let pop = ring(x - 1.7, 13.0, 4.5);
    p.size = Vec3::new(
        1.0 + 0.16 * gulp - 0.1 * pop,
        1.0 - 0.12 * gulp + 0.2 * pop,
        1.0 + 0.16 * gulp - 0.1 * pop,
    );
    p.arm_l = p.arm_l.slerp(rz(-1.0) * rx(-0.6), open);
    p.body = rx(0.14 * open - 0.1 * gulp);
    p.head = rx(-0.1 * open);
    p.hat_lift = 0.07 * arc(1.7, 2.1, x);
    p.wide = 1.0 + 0.3 * pulse(1.6, 1.7, 1.9, 2.2, x);
    let flash = (-((x - 1.68) / 0.22).powi(2)).exp();
    p.glow = candle(
        EMBER * 1.4
            + LIT * (0.6 * ramp(0.1, 0.9, x) * open + 1.6 * flash),
    );
    // It looks itself over.
    let check = pulse(2.1, 2.3, 2.6, 2.85, x);
    p.arm_l = p.arm_l.slerp(arm_l(0.5, 1.25), check);
    p.head = p.head * rx(0.26 * check) * rz(0.12 * check);
    p.swing = Vec2::new(0.25 * pop, 0.0);
    crook_out(&mut p, pulse(0.0, 0.5, 2.2, 2.8, x));
    p.key = c.t * 5.0;
    p
}
