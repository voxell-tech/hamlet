//! The sounds of what the chief does to the world by hand: small,
//! close and round, of wood, glass and a plucked tine.

use std::f32::consts::TAU;

use super::{
    Band, DT, Hiss, Low, air, bar, burst, empty, finish, glass,
    glide, hit, knock, pitch, rustle, strike, tine,
};
use crate::sound::Sound;

/// A thing chosen: a small knock, and a glass note over it.
fn select() -> Vec<f32> {
    let mut out = empty(0.5);
    knock(&mut out, 0.0, 620.0, 0.5);
    glass(&mut out, 0.004, 81.0, 0.5, 0.11);
    finish(out, 0.28, false)
}

/// The view going to a thing: air drawn in, and a note where it
/// lands.
fn focus() -> Vec<f32> {
    let mut out = empty(0.75);
    let mut hiss = Hiss(151);
    air(&mut out, &mut hiss, 0.0, 0.3, (700.0, 2600.0), 0.5);
    glass(&mut out, 0.24, 88.0, 0.6, 0.14);
    bar(&mut out, 0.24, 76.0, 0.3, 0.12);
    finish(out, 0.25, false)
}

/// A menu unrolled over the world: paper, and a tick as it stops.
fn menu() -> Vec<f32> {
    let mut out = empty(0.3);
    let mut hiss = Hiss(157);
    rustle(&mut out, &mut hiss, (0.0, 0.12), 1.0, 2600.0);
    knock(&mut out, 0.1, 1300.0, 0.25);
    finish(out, 0.25, false)
}

/// A poke: a round blip that falls a fifth.
fn poke() -> Vec<f32> {
    let mut out = empty(0.2);
    glide(&mut out, 0.0, 0.11, (pitch(81.0), pitch(74.0)), 1.0);
    knock(&mut out, 0.0, 400.0, 0.2);
    finish(out, 0.3, false)
}

/// Poked once too often: two low blips, and a rattle.
fn annoy() -> Vec<f32> {
    let mut out = empty(0.5);
    for (at, from, to) in [(0.0, 66.0, 62.0), (0.14, 64.0, 57.0)] {
        let slide = (pitch(from), pitch(to));
        glide(&mut out, at, 0.13, slide, 1.0);
        glide(
            &mut out,
            at,
            0.13,
            (slide.0 * 0.5, slide.1 * 0.5),
            0.6,
        );
        glide(
            &mut out,
            at,
            0.13,
            (slide.0 * 3.0, slide.1 * 3.0),
            0.12,
        );
    }
    for k in 0..5 {
        knock(&mut out, 0.02 + k as f32 * 0.04, 900.0, 0.1);
    }
    finish(out, 0.24, false)
}

/// A pat: two soft thumps, and a warm note under the hand.
fn pat() -> Vec<f32> {
    let mut out = empty(0.7);
    let mut hiss = Hiss(163);
    for at in [0.0, 0.14] {
        let mut low = Low::new(900.0);
        strike(
            &mut out,
            at,
            0.6,
            &[(150.0, 1.0, 0.035), (310.0, 0.5, 0.02)],
        );
        burst(&mut out, &mut hiss, at, 0.4, (0.003, 0.02), |x, _| {
            low.run(x) * 2.0
        });
    }
    tine(&mut out, 0.15, 74.0, 0.35);
    finish(out, 0.28, false)
}

/// Picked up: a note sliding up a sixth, and a little air.
fn lift() -> Vec<f32> {
    let mut out = empty(0.4);
    let mut hiss = Hiss(167);
    glide(&mut out, 0.0, 0.24, (pitch(69.0), pitch(78.0)), 1.0);
    air(&mut out, &mut hiss, 0.0, 0.2, (900.0, 2400.0), 0.12);
    finish(out, 0.2, false)
}

/// Set down: the note slides back, and a foot meets the grass.
fn set_down() -> Vec<f32> {
    let mut out = empty(0.45);
    let mut hiss = Hiss(173);
    let mut low = Low::new(1100.0);
    glide(&mut out, 0.0, 0.16, (pitch(78.0), pitch(69.0)), 0.8);
    strike(
        &mut out,
        0.15,
        1.0,
        &[(120.0, 1.0, 0.04), (250.0, 0.4, 0.025)],
    );
    burst(&mut out, &mut hiss, 0.15, 0.5, (0.004, 0.03), |x, _| {
        low.run(x) * 3.0
    });
    finish(out, 0.3, false)
}

/// A wave: two tines, the second higher.
fn wave() -> Vec<f32> {
    let mut out = empty(0.8);
    tine(&mut out, 0.0, 78.0, 0.8);
    tine(&mut out, 0.11, 81.0, 1.0);
    finish(out, 0.26, false)
}

/// A cheer: three small bars going up the chord, and a sparkle.
fn cheer() -> Vec<f32> {
    let mut out = empty(1.1);
    for (k, note) in [74.0, 78.0, 81.0].into_iter().enumerate() {
        let k = k as f32;
        bar(&mut out, k * 0.075, note, 0.8 + 0.1 * k, 0.3);
    }
    glass(&mut out, 0.24, 86.0, 0.25, 0.2);
    finish(out, 0.3, false)
}

/// Something handed over to read: paper sliding, and a tine.
fn hand() -> Vec<f32> {
    let mut out = empty(0.6);
    let mut hiss = Hiss(179);
    rustle(&mut out, &mut hiss, (0.0, 0.16), 1.0, 2000.0);
    tine(&mut out, 0.14, 83.0, 0.5);
    finish(out, 0.25, false)
}

/// The thinking light nudged: a glass note that wobbles.
fn nudge() -> Vec<f32> {
    let mut out = empty(0.7);
    let mut phase = 0.0f32;
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 * DT;
        let wobble = 1.0 + 0.015 * (TAU * 9.0 * t).sin();
        phase += TAU * pitch(83.0) * wobble * DT;
        *s = hit(t, 0.008, 0.16) * phase.sin();
    }
    glass(&mut out, 0.0, 90.0, 0.2, 0.1);
    finish(out, 0.22, false)
}

/// Two robots introduced: a note from each, then both together.
fn introduce() -> Vec<f32> {
    let mut out = empty(1.0);
    tine(&mut out, 0.0, 69.0, 0.8);
    tine(&mut out, 0.14, 74.0, 0.8);
    tine(&mut out, 0.3, 69.0, 0.5);
    tine(&mut out, 0.3, 78.0, 0.7);
    finish(out, 0.28, false)
}

/// A skill given: paper, and glass notes going up.
fn gift() -> Vec<f32> {
    let mut out = empty(1.0);
    let mut hiss = Hiss(181);
    rustle(&mut out, &mut hiss, (0.0, 0.12), 0.6, 2400.0);
    for (k, note) in [74.0, 78.0, 81.0, 86.0].into_iter().enumerate()
    {
        glass(&mut out, 0.08 + k as f32 * 0.06, note, 0.5, 0.16);
    }
    finish(out, 0.28, false)
}

/// A Wickstep going out: a breath drawn in, and glass that falls a
/// fourth as the flame goes.
fn wick_out() -> Vec<f32> {
    let mut out = empty(0.6);
    let mut hiss = Hiss(263);
    air(&mut out, &mut hiss, 0.0, 0.32, (700.0, 2600.0), 0.25);
    glass(&mut out, 0.04, 86.0, 0.5, 0.1);
    glass(&mut out, 0.2, 81.0, 0.35, 0.12);
    finish(out, 0.2, false)
}

/// A Wickstep coming in: a wick that catches, and glass that rises
/// to the keynote and its third.
fn wick_in() -> Vec<f32> {
    let mut out = empty(0.9);
    let mut hiss = Hiss(269);
    let mut band = Band::new(2.0);
    burst(&mut out, &mut hiss, 0.0, 0.3, (0.002, 0.03), |x, _| {
        band.run(x, 2400.0)
    });
    tine(&mut out, 0.02, 81.0, 0.7);
    glass(&mut out, 0.1, 86.0, 0.6, 0.22);
    glass(&mut out, 0.18, 90.0, 0.35, 0.25);
    finish(out, 0.22, false)
}

/// Clockwork switched: its pawl over three teeth, then the train
/// caught.
fn clockwork() -> Vec<f32> {
    let mut out = empty(0.5);
    let mut hiss = Hiss(271);
    let mut low = Low::new(700.0);
    for k in 0..3 {
        knock(&mut out, k as f32 * 0.035, 1500.0, 0.22);
    }
    strike(
        &mut out,
        0.13,
        1.0,
        &[
            (110.0, 1.0, 0.07),
            (320.0, 0.6, 0.04),
            (1250.0, 0.25, 0.03),
        ],
    );
    burst(&mut out, &mut hiss, 0.13, 0.5, (0.002, 0.03), |x, _| {
        low.run(x) * 3.0
    });
    finish(out, 0.3, false)
}

/// The samples of a sound of the hand.
pub(super) fn make(sound: Sound) -> Option<Vec<f32>> {
    Some(match sound {
        Sound::Select => select(),
        Sound::Focus => focus(),
        Sound::Menu => menu(),
        Sound::Poke => poke(),
        Sound::Annoy => annoy(),
        Sound::Pat => pat(),
        Sound::Lift => lift(),
        Sound::Drop => set_down(),
        Sound::Wave => wave(),
        Sound::Cheer => cheer(),
        Sound::Hand => hand(),
        Sound::Nudge => nudge(),
        Sound::Introduce => introduce(),
        Sound::Gift => gift(),
        Sound::WickOut => wick_out(),
        Sound::WickIn => wick_in(),
        Sound::Clockwork => clockwork(),
        _ => return None,
    })
}
