//! The sounds of the panels: paper for what opens, wood for what
//! moves, a tine for what is sent.

use std::f32::consts::TAU;

use super::{
    Band, DT, Hiss, Low, RATE, air, bell, burst, empty, finish,
    glass, hit, knock, pitch, rustle, strike, tine,
};
use crate::sound::Sound;

/// The pointer coming onto a button: one small, high tick.
fn tick() -> Vec<f32> {
    let mut out = empty(0.05);
    strike(&mut out, 0.0, 1.0, &[(pitch(98.0), 1.0, 0.008)]);
    finish(out, 0.16, false)
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
    burst(&mut out, &mut hiss, 0.0, 0.5, (0.001, 0.006), |x, _| {
        low.run(x)
    });
    finish(out, 0.3, false)
}

/// The action a screen is for, done: the fifth and the keynote
/// above it.
fn confirm() -> Vec<f32> {
    let mut out = empty(0.7);
    bell(&mut out, 0.0, pitch(81.0), 1.0);
    bell(&mut out, 0.07, pitch(86.0), 0.9);
    finish(out, 0.19, false)
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
    finish(out, 0.16, false)
}

/// A menu or a popup opening: air going up, and a tick.
fn pop_open() -> Vec<f32> {
    let mut out = empty(0.16);
    let mut hiss = Hiss(257);
    air(&mut out, &mut hiss, 0.0, 0.1, (700.0, 2000.0), 1.0);
    knock(&mut out, 0.07, 1500.0, 0.2);
    finish(out, 0.2, false)
}

/// A menu or a popup closing: the same air going down.
fn pop_close() -> Vec<f32> {
    let mut out = empty(0.14);
    let mut hiss = Hiss(263);
    air(&mut out, &mut hiss, 0.0, 0.09, (2400.0, 800.0), 1.0);
    finish(out, 0.16, false)
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
    finish(out, 0.24, false)
}

/// A fold opened or shut: paper creased.
fn fold() -> Vec<f32> {
    let mut out = empty(0.2);
    let mut hiss = Hiss(269);
    rustle(&mut out, &mut hiss, (0.0, 0.1), 1.0, 2200.0);
    knock(&mut out, 0.08, 1100.0, 0.15);
    finish(out, 0.2, false)
}

/// A message sent from the command line: air, and two tines going
/// with it.
fn send() -> Vec<f32> {
    let mut out = empty(0.7);
    let mut hiss = Hiss(271);
    air(&mut out, &mut hiss, 0.0, 0.2, (800.0, 3000.0), 0.3);
    tine(&mut out, 0.12, 81.0, 0.7);
    tine(&mut out, 0.2, 86.0, 0.5);
    finish(out, 0.17, false)
}

/// The card of a thing the pointer rests on: a breath of paper.
fn card() -> Vec<f32> {
    let mut out = empty(0.22);
    let mut hiss = Hiss(277);
    rustle(&mut out, &mut hiss, (0.0, 0.2), 1.0, 1800.0);
    finish(out, 0.15, false)
}

/// A panel taken hold of: a knock, and wood sliding.
fn grab() -> Vec<f32> {
    let mut out = empty(0.22);
    let mut hiss = Hiss(281);
    let mut band = Band::new(1.5);
    knock(&mut out, 0.0, 520.0, 0.6);
    burst(&mut out, &mut hiss, 0.01, 0.5, (0.02, 0.05), |x, _| {
        band.run(x, 900.0)
    });
    finish(out, 0.22, false)
}

/// A panel set in its place: wood coming to, and its latch.
fn dock() -> Vec<f32> {
    let mut out = empty(0.3);
    knock(&mut out, 0.0, 300.0, 1.0);
    strike(&mut out, 0.0, 0.6, &[(140.0, 1.0, 0.04)]);
    knock(&mut out, 0.05, 900.0, 0.3);
    finish(out, 0.3, false)
}

/// One of the window's own buttons: its frame knocked, and the
/// glass in it.
fn sash() -> Vec<f32> {
    let mut out = empty(0.3);
    knock(&mut out, 0.0, 460.0, 0.8);
    glass(&mut out, 0.0, 86.0, 0.25, 0.08);
    finish(out, 0.24, false)
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
    finish(out, 0.18, false)
}

/// The samples of a sound of the panels.
pub(super) fn make(sound: Sound) -> Option<Vec<f32>> {
    Some(match sound {
        Sound::Tick => tick(),
        Sound::Tap => tap(),
        Sound::Confirm => confirm(),
        Sound::Deny => deny(),
        Sound::PopOpen => pop_open(),
        Sound::PopClose => pop_close(),
        Sound::Tab => tab(),
        Sound::Fold => fold(),
        Sound::Send => send(),
        Sound::Card => card(),
        Sound::Grab => grab(),
        Sound::Dock => dock(),
        Sound::Sash => sash(),
        Sound::Error => error(),
        _ => return None,
    })
}
