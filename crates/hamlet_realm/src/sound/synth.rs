//! Every sound of the world, made from sines and filtered noise. The
//! same call always gives the same samples.

mod ambience;
mod chores;
mod events;
mod hand;
mod panel;

use std::f32::consts::{PI, TAU};

use super::Sound;
pub(super) use crate::meshes::hump;

/// Samples per second of every clip.
pub const RATE: u32 = 32_000;

pub(super) const DT: f32 = 1.0 / RATE as f32;

/// Seeded noise, -1 to 1.
pub(super) struct Hiss(pub(super) u32);

impl Hiss {
    pub(super) fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    /// 0 to 1.
    pub(super) fn unit(&mut self) -> f32 {
        self.next() * 0.5 + 0.5
    }
}

/// Lets through what is below `cut` hertz.
pub(super) struct Low {
    k: f32,
    y: f32,
}

impl Low {
    pub(super) fn new(cut: f32) -> Self {
        Self {
            k: 1.0 - (-TAU * cut * DT).exp(),
            y: 0.0,
        }
    }

    pub(super) fn run(&mut self, x: f32) -> f32 {
        self.y += self.k * (x - self.y);
        self.y
    }
}

/// Lets through what is near a pitch. `narrow` is how many times
/// narrower than the pitch the band is.
pub(super) struct Band {
    narrow: f32,
    low: f32,
    band: f32,
}

impl Band {
    pub(super) fn new(narrow: f32) -> Self {
        Self {
            narrow,
            low: 0.0,
            band: 0.0,
        }
    }

    pub(super) fn run(&mut self, x: f32, pitch: f32) -> f32 {
        let f = 2.0 * (PI * pitch * DT).sin();
        self.low += f * self.band;
        let high = x - self.low - self.band / self.narrow;
        self.band += f * high;
        self.band
    }
}

pub(super) fn empty(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds * RATE as f32) as usize]
}

/// A quick rise and a slow fall, 0 before `t` is 0.
pub(super) fn hit(t: f32, rise: f32, fall: f32) -> f32 {
    if t < 0.0 {
        0.0
    } else {
        (1.0 - (-t / rise).exp()) * (-t / fall).exp()
    }
}

/// Adds a struck thing to `out`: sines at `modes` (hertz, strength,
/// seconds to die away) from `at` seconds on.
pub(super) fn strike(
    out: &mut [f32],
    at: f32,
    gain: f32,
    modes: &[(f32, f32, f32)],
) {
    let from = (at * RATE as f32) as usize;
    for (i, s) in out.iter_mut().enumerate().skip(from) {
        let t = (i - from) as f32 * DT;
        let rise = 1.0 - (-t / 0.0015).exp();
        for &(pitch, strength, fall) in modes {
            *s += gain
                * strength
                * rise
                * (-t / fall).exp()
                * (TAU * pitch * t).sin();
        }
    }
}

/// Adds a burst of noise through `filter` to `out`.
pub(super) fn burst(
    out: &mut [f32],
    hiss: &mut Hiss,
    at: f32,
    gain: f32,
    (rise, fall): (f32, f32),
    mut filter: impl FnMut(f32, f32) -> f32,
) {
    let from = (at * RATE as f32) as usize;
    for (i, s) in out.iter_mut().enumerate().skip(from) {
        let t = (i - from) as f32 * DT;
        *s += gain * hit(t, rise, fall) * filter(hiss.next(), t);
    }
}

/// The pitch of a MIDI note: 69 is the A above middle C. Every
/// note here is of D major, the key of the music.
pub(super) fn pitch(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// Adds a plucked tine: the voice of a kalimba or a music box.
pub(super) fn tine(out: &mut [f32], at: f32, note: f32, gain: f32) {
    let f = pitch(note);
    // High tines ring shorter.
    let ring = 0.34 * (440.0 / f).powf(0.35);
    strike(
        out,
        at,
        gain,
        &[
            (f, 1.0, ring),
            (2.0 * f, 0.26, ring * 0.5),
            (5.4 * f, 0.14, 0.03),
        ],
    );
}

/// Adds a small struck bar, as of a glockenspiel, that rings for
/// `ring` seconds.
pub(super) fn bar(
    out: &mut [f32],
    at: f32,
    note: f32,
    gain: f32,
    ring: f32,
) {
    let f = pitch(note);
    strike(
        out,
        at,
        gain,
        &[
            (f, 1.0, ring),
            (2.756 * f, 0.36, ring * 0.5),
            (5.404 * f, 0.12, ring * 0.2),
        ],
    );
}

/// Adds a note of glass: it comes in softly and shimmers.
pub(super) fn glass(
    out: &mut [f32],
    at: f32,
    note: f32,
    gain: f32,
    ring: f32,
) {
    let f = pitch(note);
    let from = (at * RATE as f32) as usize;
    for (i, s) in out.iter_mut().enumerate().skip(from) {
        let t = (i - from) as f32 * DT;
        let shimmer = 1.0 + 0.25 * (TAU * 7.0 * t).sin();
        let x = TAU * f * t;
        *s += gain
            * hit(t, 0.012, ring)
            * shimmer
            * (x.sin()
                + 0.5 * (x * 1.003 + 1.1).sin()
                + 0.2 * (x * 2.01).sin());
    }
}

/// Adds a knock on wood.
pub(super) fn knock(out: &mut [f32], at: f32, hz: f32, gain: f32) {
    strike(
        out,
        at,
        gain,
        &[
            (hz, 1.0, 0.03),
            (hz * 2.42, 0.45, 0.016),
            (hz * 4.3, 0.18, 0.008),
        ],
    );
}

/// Adds paper moving over `span` seconds, brightest near `centre`
/// hertz.
pub(super) fn rustle(
    out: &mut [f32],
    hiss: &mut Hiss,
    (from, to): (f32, f32),
    gain: f32,
    centre: f32,
) {
    let mut band = Band::new(1.2);
    let mut soft = Low::new(3200.0);
    let mut crackle = Low::new(60.0);
    let first = (from * RATE as f32) as usize;
    let last = ((to * RATE as f32) as usize).min(out.len());
    for (i, s) in out.iter_mut().enumerate().take(last).skip(first) {
        let x = hiss.next();
        let grain = (crackle.run(x.abs()) * 2.4).min(1.0);
        *s += gain
            * hump(i as f32 * DT, from, to)
            * grain
            * soft.run(band.run(x, centre));
    }
}

/// Adds a round note that slides from one pitch to another in
/// `long` seconds.
pub(super) fn glide(
    out: &mut [f32],
    at: f32,
    long: f32,
    (from, to): (f32, f32),
    gain: f32,
) {
    let first = (at * RATE as f32) as usize;
    let count = (long * RATE as f32) as usize;
    let mut phase = 0.0f32;
    for (k, s) in out.iter_mut().skip(first).take(count).enumerate() {
        let t = k as f32 * DT;
        let u = t / long;
        phase += TAU * from * (to / from).powf(u) * DT;
        let level = (1.0 - (-t / 0.006).exp()) * (1.0 - u).powf(1.5);
        *s +=
            gain * level * (phase.sin() + 0.18 * (2.0 * phase).sin());
    }
}

/// Adds air moving for `long` seconds: noise through a band that
/// slides from one pitch to another.
pub(super) fn air(
    out: &mut [f32],
    hiss: &mut Hiss,
    at: f32,
    long: f32,
    (from, to): (f32, f32),
    gain: f32,
) {
    let mut band = Band::new(1.6);
    let first = (at * RATE as f32) as usize;
    let count = (long * RATE as f32) as usize;
    for (k, s) in out.iter_mut().skip(first).take(count).enumerate() {
        let t = k as f32 * DT;
        let centre = from * (to / from).powf(t / long);
        *s +=
            gain * hump(t, 0.0, long) * band.run(hiss.next(), centre);
    }
}

/// Takes out any offset, fades the end out and scales the loudest
/// sample to `peak`.
pub(super) fn finish(
    mut out: Vec<f32>,
    peak: f32,
    looped: bool,
) -> Vec<f32> {
    let mean = out.iter().sum::<f32>() / out.len().max(1) as f32;
    let tail = (0.008 * RATE as f32) as usize;
    let n = out.len();
    for (i, s) in out.iter_mut().enumerate() {
        *s -= mean;
        if !looped && i + tail >= n {
            *s *= (n - i) as f32 / tail as f32;
        }
    }
    let loudest = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if loudest > 0.0 {
        for s in &mut out {
            *s *= peak / loudest;
        }
    }
    out
}

/// One note of a small bell.
pub(super) fn bell(out: &mut [f32], at: f32, pitch: f32, gain: f32) {
    strike(
        out,
        at,
        gain,
        &[
            (pitch, 1.0, 0.55),
            // A second, slightly off, so the note shimmers.
            (pitch + 0.7, 0.6, 0.5),
            (pitch * 2.0, 0.3, 0.3),
            (pitch * 3.01, 0.1, 0.18),
            (pitch * 4.2, 0.05, 0.1),
        ],
    );
}

/// The samples of a sound.
pub fn make(sound: Sound) -> Vec<f32> {
    chores::make(sound)
        .or_else(|| ambience::make(sound))
        .or_else(|| hand::make(sound))
        .or_else(|| events::make(sound))
        .or_else(|| panel::make(sound))
        .unwrap_or_default()
}
