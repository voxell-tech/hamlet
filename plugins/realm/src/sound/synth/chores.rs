//! The sounds of robots about their houses: feet, tools, doors and
//! paper.

use std::f32::consts::TAU;

use crate::build::mesh::hump;
use crate::sound::kinds::Sound;
use crate::sound::synth::{
    Band, DT, Hiss, Low, burst, empty, finish, hit, strike,
};

/// A foot on grass or on a yard's paving.
fn step() -> Vec<f32> {
    let mut out = empty(0.18);
    let mut hiss = Hiss(11);
    let (mut low, mut soft) = (Low::new(520.0), Low::new(520.0));
    // The brush of grass, with nothing sharp in it.
    burst(&mut out, &mut hiss, 0.0, 1.0, (0.012, 0.045), |x, _| {
        soft.run(low.run(x)) * 3.0
    });
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 * DT;
        // A padded thump that sinks in pitch as the foot settles.
        let pitch = 82.0 + 36.0 * (-t / 0.03).exp();
        *s += 0.5 * hit(t, 0.008, 0.045) * (TAU * pitch * t).sin();
    }
    finish(out, 0.32, false)
}

/// A foot on a road: a soft knock, as of a wooden sole on packed
/// earth.
fn road_step() -> Vec<f32> {
    let mut out = empty(0.16);
    let mut hiss = Hiss(23);
    let (mut low, mut soft) = (Low::new(900.0), Low::new(900.0));
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 * DT;
        let body = hit(t, 0.005, 0.03) * (TAU * 147.0 * t).sin();
        let knock = hit(t, 0.004, 0.014) * (TAU * 294.0 * t).sin();
        *s += 0.6 * body + 0.22 * knock;
    }
    burst(&mut out, &mut hiss, 0.0, 0.5, (0.006, 0.02), |x, _| {
        soft.run(low.run(x)) * 2.0
    });
    finish(out, 0.32, false)
}

/// A mallet on wood.
fn hammer() -> Vec<f32> {
    let mut out = empty(0.28);
    let mut hiss = Hiss(37);
    let mut low = Low::new(3000.0);
    strike(
        &mut out,
        0.0,
        1.0,
        &[
            (520.0, 1.0, 0.045),
            (1210.0, 0.5, 0.028),
            (2050.0, 0.22, 0.014),
            (150.0, 0.5, 0.04),
        ],
    );
    burst(&mut out, &mut hiss, 0.0, 0.4, (0.0005, 0.004), |x, _| {
        low.run(x)
    });
    finish(out, 0.36, false)
}

/// One turn of the engine's wheel: a creak and two catches of its
/// pawl.
fn wheel() -> Vec<f32> {
    let mut out = empty(0.34);
    let mut hiss = Hiss(41);
    let mut wood = Band::new(3.0);
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 * DT;
        // The creak: a low buzz that wavers as the wood sticks and
        // slips.
        let pitch = 92.0 + 14.0 * (TAU * 9.0 * t).sin();
        let buzz = (1..=5)
            .map(|h| (TAU * pitch * h as f32 * t).sin() / h as f32)
            .sum::<f32>();
        *s += 0.3 * hit(t, 0.03, 0.09) * wood.run(buzz, 620.0);
    }
    for at in [0.02, 0.15] {
        let mut band = Band::new(3.0);
        strike(&mut out, at, 0.35, &[(1500.0, 1.0, 0.006)]);
        burst(
            &mut out,
            &mut hiss,
            at,
            0.4,
            (0.0005, 0.005),
            |x, _| band.run(x, 2400.0),
        );
    }
    finish(out, 0.4, false)
}

/// A puff of steam.
fn steam() -> Vec<f32> {
    let mut out = empty(0.7);
    let mut hiss = Hiss(53);
    let mut band = Band::new(0.8);
    let mut low = Low::new(5000.0);
    burst(&mut out, &mut hiss, 0.0, 1.0, (0.025, 0.16), |x, t| {
        // It opens bright and closes dull.
        low.run(band.run(x, 1400.0 + 1200.0 * (-t / 0.12).exp()))
    });
    finish(out, 0.3, false)
}

/// A quill across paper: a few short strokes.
fn quill() -> Vec<f32> {
    let mut out = empty(0.5);
    let mut hiss = Hiss(67);
    let mut band = Band::new(1.5);
    let mut low = Low::new(3500.0);
    let strokes = [
        (0.01, 0.07),
        (0.09, 0.15),
        (0.17, 0.21),
        (0.25, 0.33),
        (0.36, 0.43),
    ];
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 * DT;
        let press = strokes
            .iter()
            .map(|&(from, to)| hump(t, from, to))
            .sum::<f32>();
        *s = press * low.run(band.run(hiss.next(), 2400.0));
    }
    finish(out, 0.22, false)
}

/// A kite's cloth pulled taut in the wind.
fn kite() -> Vec<f32> {
    let mut out = empty(0.95);
    let mut hiss = Hiss(71);
    let mut low = Low::new(700.0);
    let mut whistle = Band::new(6.0);
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 * DT;
        let x = hiss.next();
        let flap =
            0.3 + 0.7 * (TAU * 11.0 * t).sin().max(0.0).powi(2);
        let swell = hit(t, 0.12, 0.3);
        *s = swell
            * (flap * low.run(x) * 3.0
                + 0.15 * whistle.run(x, 1100.0));
    }
    finish(out, 0.3, false)
}

/// A door: its latch, then the wood coming to.
fn door() -> Vec<f32> {
    let mut out = empty(0.42);
    let mut hiss = Hiss(83);
    let mut latch = Band::new(3.0);
    let mut low = Low::new(500.0);
    strike(&mut out, 0.0, 0.3, &[(900.0, 1.0, 0.01)]);
    burst(&mut out, &mut hiss, 0.0, 0.3, (0.0005, 0.006), |x, _| {
        latch.run(x, 2000.0)
    });
    strike(
        &mut out,
        0.06,
        1.0,
        &[(96.0, 0.5, 0.06), (240.0, 0.8, 0.05), (530.0, 0.4, 0.025)],
    );
    burst(&mut out, &mut hiss, 0.06, 0.8, (0.002, 0.03), |x, _| {
        low.run(x) * 3.0
    });
    finish(out, 0.55, false)
}

/// Paper changing hands.
fn letter() -> Vec<f32> {
    let mut out = empty(0.36);
    let mut hiss = Hiss(101);
    let mut band = Band::new(1.2);
    let mut soft = Low::new(4000.0);
    let mut crackle = Low::new(60.0);
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 * DT;
        let x = hiss.next();
        let rustle = hump(t, 0.01, 0.09) + 0.8 * hump(t, 0.15, 0.3);
        let grain = (crackle.run(x.abs()) * 2.4).min(1.0);
        *s = rustle * grain * soft.run(band.run(x, 2200.0));
    }
    finish(out, 0.25, false)
}

/// The samples of a sound of the robots' chores.
pub(super) fn make(sound: Sound) -> Option<Vec<f32>> {
    Some(match sound {
        Sound::Step => step(),
        Sound::RoadStep => road_step(),
        Sound::Hammer => hammer(),
        Sound::Wheel => wheel(),
        Sound::Steam => steam(),
        Sound::Quill => quill(),
        Sound::Kite => kite(),
        Sound::Door => door(),
        Sound::Letter => letter(),
        _ => return None,
    })
}
