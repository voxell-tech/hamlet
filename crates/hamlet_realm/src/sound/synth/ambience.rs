//! The loop of the ambience: wind.

use std::f32::consts::TAU;

use hamlet_core::sound::Sound;

use super::{Band, DT, Hiss, Low, RATE, empty, finish};

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
    (sound == Sound::Wind).then(wind)
}
