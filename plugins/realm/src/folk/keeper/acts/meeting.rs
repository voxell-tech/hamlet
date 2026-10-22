//! Listening at its door for another's answer.

use crate::folk::keeper::acts::Ctx;
use crate::folk::keeper::acts::parts::{
    blink, breathe, crook_out, current, shift_weight,
};
use crate::folk::keeper::pose::{LIT, Pose, rx, rz};

pub(super) fn listen(c: &Ctx) -> Pose {
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
