//! The interface's sounds, made from sines and filtered noise: paper
//! for what opens, wood for what moves, a tine for what is sent. The
//! same call always gives the same samples. Every note is of D major.

use std::f32::consts::{PI, TAU};

use crate::sounds::InterfaceSound;

/// Samples per second of every clip.
pub const RATE: u32 = 32_000;

const DT: f32 = 1.0 / RATE as f32;

/// The samples of `sound`, one channel at [`RATE`].
pub fn make(sound: InterfaceSound) -> Vec<f32> {
    match sound {
        InterfaceSound::Tick => tick(),
        InterfaceSound::Tap => tap(),
        InterfaceSound::Confirm => confirm(),
        InterfaceSound::Deny => deny(),
        InterfaceSound::PopOpen => pop_open(),
        InterfaceSound::PopClose => pop_close(),
        InterfaceSound::Tab => tab(),
        InterfaceSound::Fold => fold(),
        InterfaceSound::Send => send(),
        InterfaceSound::Grab => grab(),
        InterfaceSound::Dock => dock(),
        InterfaceSound::Sash => sash(),
        InterfaceSound::Error => error(),
    }
}

/// The pointer coming onto a button: one small, high tick.
fn tick() -> Vec<f32> {
    let mut out = empty(0.05);
    strike(&mut out, 0.0, 1.0, &[(pitch(98.0), 1.0, 0.008)]);
    finish(out, 0.16)
}

/// A button going down: a soft knock with a little body.
fn tap() -> Vec<f32> {
    let mut out = empty(0.09);
    let mut hiss = Hiss(131);
    let mut low = Low::new(2600.0);
    strike(
        &mut out,
        0.0,
        1.0,
        &[(420.0, 1.0, 0.018), (1260.0, 0.35, 0.01)],
    );
    burst(&mut out, &mut hiss, 0.0, 0.5, (0.001, 0.006), |x| {
        low.run(x)
    });
    finish(out, 0.3)
}

/// The action a screen is for, done: the fifth and the keynote
/// above it.
fn confirm() -> Vec<f32> {
    let mut out = empty(0.7);
    bell(&mut out, 0.0, pitch(81.0), 1.0);
    bell(&mut out, 0.07, pitch(86.0), 0.9);
    finish(out, 0.19)
}

/// Something refused or stopped: two dull notes going down a third.
fn deny() -> Vec<f32> {
    let mut out = empty(0.36);
    for (at, note) in [(0.0, 57.0), (0.1, 54.0)] {
        let from = (at * RATE as f32) as usize;
        for (i, s) in out.iter_mut().enumerate().skip(from) {
            let t = (i - from) as f32 * DT;
            let phase = TAU * pitch(note) * t;
            let note = phase.sin() + 0.35 * (3.0 * phase).sin();
            *s += hit(t, 0.004, 0.07) * (note * 1.2).tanh();
        }
    }
    finish(out, 0.16)
}

/// A menu or a popup opening: air going up, and a tick.
fn pop_open() -> Vec<f32> {
    let mut out = empty(0.16);
    let mut hiss = Hiss(257);
    air(&mut out, &mut hiss, 0.0, 0.1, (700.0, 2000.0), 1.0);
    knock(&mut out, 0.07, 1500.0, 0.2);
    finish(out, 0.2)
}

/// A menu or a popup closing: the same air going down.
fn pop_close() -> Vec<f32> {
    let mut out = empty(0.14);
    let mut hiss = Hiss(263);
    air(&mut out, &mut hiss, 0.0, 0.09, (2400.0, 800.0), 1.0);
    finish(out, 0.16)
}

/// A tab brought to the front: a small hollow click, on the fifth.
fn tab() -> Vec<f32> {
    let mut out = empty(0.1);
    let f = pitch(81.0);
    strike(
        &mut out,
        0.0,
        1.0,
        &[(f, 1.0, 0.02), (f * 2.76, 0.3, 0.01)],
    );
    knock(&mut out, 0.0, 700.0, 0.4);
    finish(out, 0.24)
}

/// A fold opened or shut: paper creased.
fn fold() -> Vec<f32> {
    let mut out = empty(0.2);
    let mut hiss = Hiss(269);
    rustle(&mut out, &mut hiss, (0.0, 0.1), 1.0, 2200.0);
    knock(&mut out, 0.08, 1100.0, 0.15);
    finish(out, 0.2)
}

/// A message sent: air, and two tines going with it.
fn send() -> Vec<f32> {
    let mut out = empty(0.7);
    let mut hiss = Hiss(271);
    air(&mut out, &mut hiss, 0.0, 0.2, (800.0, 3000.0), 0.3);
    tine(&mut out, 0.12, 81.0, 0.7);
    tine(&mut out, 0.2, 86.0, 0.5);
    finish(out, 0.17)
}

/// A panel taken hold of: a knock, and wood sliding.
fn grab() -> Vec<f32> {
    let mut out = empty(0.22);
    let mut hiss = Hiss(281);
    let mut band = Band::new(1.5);
    knock(&mut out, 0.0, 520.0, 0.6);
    burst(&mut out, &mut hiss, 0.01, 0.5, (0.02, 0.05), |x| {
        band.run(x, 900.0)
    });
    finish(out, 0.22)
}

/// A panel set in its place: wood coming to, and its latch.
fn dock() -> Vec<f32> {
    let mut out = empty(0.3);
    knock(&mut out, 0.0, 300.0, 1.0);
    strike(&mut out, 0.0, 0.6, &[(140.0, 1.0, 0.04)]);
    knock(&mut out, 0.05, 900.0, 0.3);
    finish(out, 0.3)
}

/// One of the window's own actions: its frame knocked, and the glass
/// in it.
fn sash() -> Vec<f32> {
    let mut out = empty(0.3);
    knock(&mut out, 0.0, 460.0, 0.8);
    glass(&mut out, 0.0, 86.0, 0.25, 0.08);
    finish(out, 0.24)
}

/// Something went wrong: two muted notes of a wooden bar, the
/// second a fourth lower.
fn error() -> Vec<f32> {
    let mut out = empty(0.6);
    for (at, note) in [(0.0, 64.0), (0.13, 59.0)] {
        let f = pitch(note);
        strike(
            &mut out,
            at,
            1.0,
            &[(f, 1.0, 0.09), (f * 3.0, 0.2, 0.03)],
        );
    }
    finish(out, 0.18)
}

/// Seeded noise, -1 to 1.
struct Hiss(u32);

impl Hiss {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// Lets through what is below `cut` hertz.
struct Low {
    k: f32,
    y: f32,
}

impl Low {
    fn new(cut: f32) -> Self {
        Self {
            k: 1.0 - (-TAU * cut * DT).exp(),
            y: 0.0,
        }
    }

    fn run(&mut self, x: f32) -> f32 {
        self.y += self.k * (x - self.y);
        self.y
    }
}

/// Lets through what is near a pitch. `narrow` is how many times
/// narrower than the pitch the band is.
struct Band {
    narrow: f32,
    low: f32,
    band: f32,
}

impl Band {
    fn new(narrow: f32) -> Self {
        Self {
            narrow,
            low: 0.0,
            band: 0.0,
        }
    }

    fn run(&mut self, x: f32, pitch: f32) -> f32 {
        let f = 2.0 * (PI * pitch * DT).sin();
        self.low += f * self.band;
        let high = x - self.low - self.band / self.narrow;
        self.band += f * high;
        self.band
    }
}

fn empty(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds * RATE as f32) as usize]
}

/// A rise and fall over `from` to `to`, 0 outside it.
fn hump(x: f32, from: f32, to: f32) -> f32 {
    if x <= from || x >= to {
        return 0.0;
    }
    0.5 - 0.5 * (TAU * (x - from) / (to - from)).cos()
}

/// A quick rise and a slow fall, 0 before `t` is 0.
fn hit(t: f32, rise: f32, fall: f32) -> f32 {
    if t < 0.0 {
        0.0
    } else {
        (1.0 - (-t / rise).exp()) * (-t / fall).exp()
    }
}

/// Adds a struck thing to `out`: sines at `modes` (hertz, strength,
/// seconds to die away) from `at` seconds on.
fn strike(
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
fn burst(
    out: &mut [f32],
    hiss: &mut Hiss,
    at: f32,
    gain: f32,
    (rise, fall): (f32, f32),
    mut filter: impl FnMut(f32) -> f32,
) {
    let from = (at * RATE as f32) as usize;
    for (i, s) in out.iter_mut().enumerate().skip(from) {
        let t = (i - from) as f32 * DT;
        *s += gain * hit(t, rise, fall) * filter(hiss.next());
    }
}

/// The pitch of a MIDI note: 69 is the A above middle C.
fn pitch(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// Adds a plucked tine, as of a kalimba.
fn tine(out: &mut [f32], at: f32, note: f32, gain: f32) {
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

/// Adds a note of glass: it comes in softly and shimmers.
fn glass(out: &mut [f32], at: f32, note: f32, gain: f32, ring: f32) {
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
fn knock(out: &mut [f32], at: f32, hz: f32, gain: f32) {
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

/// Adds paper moving from `from` to `to` seconds, brightest near
/// `centre` hertz.
fn rustle(
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

/// Adds air moving for `long` seconds: noise through a band that
/// slides from one pitch to another.
fn air(
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

/// One note of a small bell.
fn bell(out: &mut [f32], at: f32, pitch: f32, gain: f32) {
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

/// Takes out any offset, fades the end out and scales the loudest
/// sample to `peak`.
fn finish(mut out: Vec<f32>, peak: f32) -> Vec<f32> {
    let mean = out.iter().sum::<f32>() / out.len().max(1) as f32;
    let tail = (0.008 * RATE as f32) as usize;
    let n = out.len();
    for (i, s) in out.iter_mut().enumerate() {
        *s -= mean;
        if i + tail >= n {
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
