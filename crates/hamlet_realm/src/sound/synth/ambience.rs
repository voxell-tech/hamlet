//! The loops of the ambience: birds, an owl and wind.

use std::f32::consts::TAU;

use super::{Band, DT, Hiss, Low, RATE, empty, finish, hump};
use crate::sound::Sound;

/// Adds `voice` to a loop from `at` seconds on, for `long` seconds,
/// carrying on from the start where it runs off the end.
fn sing(
    out: &mut [f32],
    at: f32,
    long: f32,
    mut voice: impl FnMut(f32) -> f32,
) {
    let n = out.len();
    let from = (at * RATE as f32) as usize;
    for k in 0..(long * RATE as f32) as usize {
        out[(from + k) % n] += voice(k as f32 * DT);
    }
}

/// Small birds, far enough off to be soft. A loop.
fn birds() -> Vec<f32> {
    let mut out = empty(14.0);
    let mut hiss = Hiss(113);
    let mut at = 0.3;
    while at < 13.5 {
        let loud = 0.2 + 0.8 * hiss.unit().powi(2);
        let high = 0.9 + 0.25 * hiss.unit();
        match (hiss.unit() * 3.0) as u32 {
            // A few quick falling notes.
            0 => {
                let notes = 3 + (hiss.unit() * 3.0) as u32;
                for k in 0..notes {
                    sing(&mut out, at + k as f32 * 0.1, 0.07, |t| {
                        let pitch =
                            high * (4200.0 - 1200.0 * t / 0.07);
                        loud * hump(t, 0.0, 0.07)
                            * (TAU * pitch * t).sin()
                    });
                }
            }
            // Two whistled notes, the second higher.
            1 => {
                for (k, pitch) in [2600.0, 3300.0].iter().enumerate()
                {
                    sing(&mut out, at + k as f32 * 0.16, 0.13, |t| {
                        let waver =
                            1.0 + 0.012 * (TAU * 30.0 * t).sin();
                        loud * hump(t, 0.0, 0.13)
                            * (TAU * pitch * high * waver * t).sin()
                    });
                }
            }
            // A trill.
            _ => sing(&mut out, at, 0.4, |t| {
                let beat = 0.5 + 0.5 * (TAU * 26.0 * t).sin();
                loud * 0.8
                    * hump(t, 0.0, 0.4)
                    * beat
                    * (TAU * 2900.0 * high * t).sin()
            }),
        }
        at += 1.8 + 3.2 * hiss.unit();
    }
    finish(out, 0.3, true)
}

/// A far owl, a few soft hoots with long quiet between. A loop.
fn owl() -> Vec<f32> {
    let long = 24.0;
    let mut out = empty(long);
    let mut hiss = Hiss(127);
    for at in [3.0, 11.5, 18.0] {
        let at = at + 2.0 * hiss.unit();
        let loud = 0.5 + 0.5 * hiss.unit();
        // A short hoot, a pause, then a longer one that sinks.
        for (k, (start, hold)) in
            [(0.0, 0.22), (0.45, 0.5)].into_iter().enumerate()
        {
            sing(&mut out, at + start, hold, |t| {
                let pitch = 420.0 - 40.0 * k as f32 * t / hold;
                loud * hump(t, 0.0, hold)
                    * ((TAU * pitch * t).sin()
                        + 0.3 * (TAU * 2.0 * pitch * t).sin())
            });
        }
    }
    finish(out, 0.2, true)
}

/// Wind in the trees. A loop.
fn wind() -> Vec<f32> {
    let long = 12.0;
    let lap = 1.0;
    let mut out = empty(long + lap);
    let mut hiss = Hiss(131);
    let (mut a, mut b) = (Low::new(520.0), Low::new(520.0));
    let mut leaves = Band::new(1.2);
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 * DT;
        let x = hiss.next();
        // Gusts that come round with the loop.
        let turn = TAU * t / long;
        let gust = 0.55
            + 0.25 * (turn + 1.0).sin()
            + 0.2 * (3.0 * turn + 2.0).sin();
        let rush = 0.25 * leaves.run(x, 1500.0 + 500.0 * turn.sin());
        *s = gust * (4.0 * b.run(a.run(x)) + rush * gust);
    }
    // The end is folded over the start, so the loop has no seam.
    let n = (long * RATE as f32) as usize;
    let fold = out.len() - n;
    for i in 0..fold {
        let k = i as f32 / fold as f32;
        out[i] = out[i] * k.sqrt() + out[n + i] * (1.0 - k).sqrt();
    }
    out.truncate(n);
    finish(out, 0.3, true)
}

/// The samples of a loop of the ambience.
pub(super) fn make(sound: Sound) -> Option<Vec<f32>> {
    Some(match sound {
        Sound::Birds => birds(),
        Sound::Owl => owl(),
        Sound::Wind => wind(),
        _ => return None,
    })
}
