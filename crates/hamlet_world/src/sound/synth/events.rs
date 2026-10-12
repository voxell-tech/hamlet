//! The sounds of what the world does of itself: houses coming and
//! going, the small magic about them, roads, balloons, the turn of
//! the day and the lives of the agents.

use std::f32::consts::TAU;

use super::{
    Band, DT, Hiss, Low, air, bar, bell, burst, empty, finish, glass,
    glide, hit, hump, knock, pitch, rustle, strike, tine,
};
use crate::sound::Sound;

/// A house standing up: timber that creaks as it climbs, and the
/// air it moves.
fn house_rise() -> Vec<f32> {
    let mut out = empty(0.75);
    let mut hiss = Hiss(191);
    let mut wood = Band::new(3.0);
    let mut phase = 0.0f32;
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 * DT;
        let u = (t / 0.55).min(1.0);
        // The creak climbs with the house, sticking and slipping.
        let hz = 80.0 + 90.0 * u + 9.0 * (TAU * 13.0 * t).sin();
        phase += TAU * hz * DT;
        let buzz = (1..=5)
            .map(|h| (phase * h as f32).sin() / h as f32)
            .sum::<f32>();
        *s += 0.5
            * hump(t, 0.0, 0.62)
            * wood.run(buzz, 480.0 + 420.0 * u);
    }
    air(&mut out, &mut hiss, 0.0, 0.6, (300.0, 1500.0), 0.35);
    finish(out, 0.4, false)
}

/// A house come to rest: a soft thump, and a low bar with its fifth.
fn house_settle() -> Vec<f32> {
    let mut out = empty(1.3);
    let mut hiss = Hiss(193);
    let mut low = Low::new(500.0);
    strike(
        &mut out,
        0.0,
        1.0,
        &[(98.0, 0.7, 0.07), (196.0, 0.5, 0.05)],
    );
    burst(&mut out, &mut hiss, 0.0, 0.5, (0.003, 0.03), |x, _| {
        low.run(x) * 3.0
    });
    bar(&mut out, 0.01, 62.0, 0.45, 0.5);
    bar(&mut out, 0.03, 69.0, 0.3, 0.45);
    finish(out, 0.32, false)
}

/// A house about to fall: its timbers rattling.
fn house_shudder() -> Vec<f32> {
    let mut out = empty(0.45);
    let mut hiss = Hiss(197);
    let mut low = Low::new(300.0);
    for k in 0..8 {
        let at = k as f32 * 0.043 + 0.008 * hiss.unit();
        let hz = 300.0 + 220.0 * hiss.unit();
        knock(&mut out, at, hz, 0.35 + 0.07 * k as f32);
    }
    burst(&mut out, &mut hiss, 0.0, 0.6, (0.12, 0.14), |x, _| {
        low.run(x) * 3.0
    });
    finish(out, 0.32, false)
}

/// A house fallen flat: a thud, and its dust.
fn house_fall() -> Vec<f32> {
    let mut out = empty(0.9);
    let mut hiss = Hiss(199);
    let mut low = Low::new(450.0);
    let mut dust = Band::new(0.9);
    let mut soft = Low::new(2600.0);
    strike(
        &mut out,
        0.0,
        1.0,
        &[(70.0, 1.0, 0.09), (140.0, 0.6, 0.06), (260.0, 0.3, 0.03)],
    );
    burst(&mut out, &mut hiss, 0.0, 0.7, (0.004, 0.05), |x, _| {
        low.run(x) * 3.0
    });
    burst(&mut out, &mut hiss, 0.03, 0.22, (0.06, 0.22), |x, _| {
        soft.run(dust.run(x, 1800.0))
    });
    finish(out, 0.36, false)
}

/// A circle of runes opening: glass notes coming in one over
/// another.
fn rune() -> Vec<f32> {
    let mut out = empty(1.5);
    let mut hiss = Hiss(211);
    for (k, note) in [74.0, 81.0, 88.0, 90.0].into_iter().enumerate()
    {
        let k = k as f32;
        glass(&mut out, k * 0.07, note, 0.5 - 0.05 * k, 0.4);
    }
    air(&mut out, &mut hiss, 0.0, 0.5, (1500.0, 4200.0), 0.05);
    finish(out, 0.26, false)
}

/// Embers: small crackles that thin out as they rise.
fn embers() -> Vec<f32> {
    let mut out = empty(1.4);
    let mut hiss = Hiss(223);
    let mut bed = Low::new(900.0);
    // The rest of the fire breathes out under them.
    burst(&mut out, &mut hiss, 0.0, 0.3, (0.08, 0.35), |x, _| {
        bed.run(x)
    });
    let mut at = 0.02;
    while at < 1.2 {
        let loud = (1.0 - at / 1.3) * (0.4 + 0.6 * hiss.unit());
        let hz = 1800.0 + 2200.0 * hiss.unit();
        let mut band = Band::new(2.0);
        burst(
            &mut out,
            &mut hiss,
            at,
            loud,
            (0.0005, 0.009),
            |x, _| band.run(x, hz),
        );
        at += (0.02 + 0.07 * hiss.unit()) * (1.0 + at);
    }
    finish(out, 0.3, false)
}

/// A house merged away: its motes lifting off, as notes that slide
/// upward.
fn merge_away() -> Vec<f32> {
    let mut out = empty(1.2);
    let mut hiss = Hiss(227);
    let slides =
        [(0.0, 74.0, 86.0), (0.1, 81.0, 93.0), (0.2, 78.0, 90.0)];
    for (at, from, to) in slides {
        glide(&mut out, at, 0.8, (pitch(from), pitch(to)), 0.5);
    }
    air(&mut out, &mut hiss, 0.0, 0.9, (800.0, 3200.0), 0.08);
    finish(out, 0.28, false)
}

/// The motes arriving: notes falling into a chord, and a low bloom
/// under them.
fn merge_land() -> Vec<f32> {
    let mut out = empty(1.6);
    for (k, note) in [93.0, 90.0, 86.0, 81.0].into_iter().enumerate()
    {
        glass(&mut out, k as f32 * 0.06, note, 0.5, 0.3);
    }
    bar(&mut out, 0.26, 74.0, 0.5, 0.6);
    strike(&mut out, 0.26, 0.5, &[(pitch(50.0), 1.0, 0.25)]);
    finish(out, 0.32, false)
}

/// A road laid: stones set down one after another, and a small bar
/// as the last goes in.
fn road_laid() -> Vec<f32> {
    let mut out = empty(0.8);
    let mut hiss = Hiss(229);
    for k in 0..5 {
        let at = k as f32 * 0.085 + 0.01 * hiss.unit();
        let mut band = Band::new(2.0);
        knock(&mut out, at, 340.0 + 45.0 * k as f32, 0.7);
        burst(&mut out, &mut hiss, at, 0.3, (0.001, 0.01), |x, _| {
            band.run(x, 1500.0)
        });
    }
    bar(&mut out, 0.36, 81.0, 0.22, 0.3);
    finish(out, 0.3, false)
}

/// A road lighting as a message sets out on it: glass that swells
/// and goes.
fn road_glow() -> Vec<f32> {
    let mut out = empty(0.9);
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 * DT;
        let shimmer = 1.0 + 0.2 * (TAU * 5.0 * t).sin();
        *s = hump(t, 0.0, 0.85)
            * shimmer
            * ((TAU * pitch(81.0) * t).sin()
                + 0.6 * (TAU * pitch(88.0) * t).sin());
    }
    finish(out, 0.15, false)
}

/// A balloon for speech: a small pop, and its paper.
fn speak() -> Vec<f32> {
    let mut out = empty(0.22);
    let mut hiss = Hiss(233);
    glide(&mut out, 0.0, 0.05, (520.0, 980.0), 1.0);
    rustle(&mut out, &mut hiss, (0.03, 0.14), 0.35, 2600.0);
    finish(out, 0.22, false)
}

/// A balloon for a thought: two round bubbles, the second smaller.
fn think() -> Vec<f32> {
    let mut out = empty(0.3);
    glide(&mut out, 0.0, 0.06, (330.0, 440.0), 1.0);
    glide(&mut out, 0.1, 0.05, (440.0, 590.0), 0.6);
    finish(out, 0.18, false)
}

/// The light of an agent that has begun to think: glass, far off.
fn muse() -> Vec<f32> {
    let mut out = empty(1.0);
    glass(&mut out, 0.0, 81.0, 1.0, 0.3);
    glass(&mut out, 0.02, 88.0, 0.3, 0.25);
    finish(out, 0.16, false)
}

/// Day breaking: three bars going up in fifths.
fn dawn() -> Vec<f32> {
    let mut out = empty(1.9);
    for (k, note) in [74.0, 81.0, 88.0].into_iter().enumerate() {
        bar(&mut out, k as f32 * 0.16, note, 0.8, 0.7);
    }
    glass(&mut out, 0.34, 93.0, 0.2, 0.5);
    finish(out, 0.25, false)
}

/// Night falling: three bars coming down to the sixth.
fn dusk() -> Vec<f32> {
    let mut out = empty(1.9);
    for (k, note) in [81.0, 78.0, 71.0].into_iter().enumerate() {
        bar(&mut out, k as f32 * 0.2, note, 0.8, 0.8);
    }
    finish(out, 0.25, false)
}

/// An agent made: the first two notes of the theme, on a music box.
fn born() -> Vec<f32> {
    let mut out = empty(1.2);
    tine(&mut out, 0.0, 78.0, 0.9);
    tine(&mut out, 0.16, 81.0, 1.0);
    bar(&mut out, 0.16, 93.0, 0.1, 0.3);
    finish(out, 0.3, false)
}

/// An agent removed: two tines going down, and a breath.
fn gone() -> Vec<f32> {
    let mut out = empty(1.2);
    let mut hiss = Hiss(239);
    tine(&mut out, 0.0, 69.0, 0.9);
    tine(&mut out, 0.2, 66.0, 0.8);
    air(&mut out, &mut hiss, 0.1, 0.5, (1800.0, 600.0), 0.06);
    finish(out, 0.28, false)
}

/// An agent restored: home again, up the fifth to the keynote.
fn restored() -> Vec<f32> {
    let mut out = empty(1.4);
    tine(&mut out, 0.0, 74.0, 0.8);
    tine(&mut out, 0.13, 81.0, 0.9);
    tine(&mut out, 0.26, 86.0, 1.0);
    bar(&mut out, 0.26, 86.0, 0.25, 0.5);
    finish(out, 0.32, false)
}

/// A vote called: two knocks of a gavel, and a small bell.
fn vote_called() -> Vec<f32> {
    let mut out = empty(1.1);
    for (at, gain) in [(0.0, 1.0), (0.16, 0.8)] {
        knock(&mut out, at, 360.0, gain);
        strike(&mut out, at, gain * 0.5, &[(130.0, 1.0, 0.04)]);
    }
    bell(&mut out, 0.3, pitch(81.0), 0.5);
    finish(out, 0.35, false)
}

/// A ballot cast: a slip of paper dropped into a box.
fn ballot() -> Vec<f32> {
    let mut out = empty(0.4);
    let mut hiss = Hiss(241);
    rustle(&mut out, &mut hiss, (0.0, 0.12), 0.8, 2800.0);
    knock(&mut out, 0.13, 240.0, 0.9);
    strike(&mut out, 0.13, 0.5, &[(120.0, 1.0, 0.04)]);
    finish(out, 0.25, false)
}

/// A new mayor: tines up the home chord, and a bell on top.
fn mayor() -> Vec<f32> {
    let mut out = empty(1.6);
    for (k, note) in [62.0, 69.0, 74.0, 78.0].into_iter().enumerate()
    {
        tine(&mut out, k as f32 * 0.1, note, 0.8);
    }
    bell(&mut out, 0.4, pitch(86.0), 0.5);
    finish(out, 0.35, false)
}

/// A hamlet founded: the home chord rolled upward on small bars.
fn founded() -> Vec<f32> {
    let mut out = empty(1.95);
    tine(&mut out, 0.0, 62.0, 0.6);
    for (k, note) in [74.0, 81.0, 86.0, 90.0].into_iter().enumerate()
    {
        bar(&mut out, k as f32 * 0.07, note, 0.8, 0.9);
    }
    glass(&mut out, 0.3, 93.0, 0.12, 0.6);
    finish(out, 0.38, false)
}

/// The answer was yes: up the chord from the fifth, the question of
/// the chime closed.
fn allowed() -> Vec<f32> {
    let mut out = empty(1.3);
    tine(&mut out, 0.0, 81.0, 0.8);
    tine(&mut out, 0.1, 86.0, 1.0);
    tine(&mut out, 0.2, 90.0, 0.8);
    bar(&mut out, 0.2, 90.0, 0.2, 0.5);
    finish(out, 0.35, false)
}

/// The answer was no: two tines down a third, and a low note under
/// the second.
fn denied() -> Vec<f32> {
    let mut out = empty(0.9);
    tine(&mut out, 0.0, 74.0, 0.9);
    tine(&mut out, 0.14, 71.0, 0.9);
    strike(&mut out, 0.14, 0.4, &[(pitch(47.0), 1.0, 0.12)]);
    finish(out, 0.32, false)
}

/// Everything stopped: three muted notes down to the low keynote,
/// and a hand laid on them.
fn stopped() -> Vec<f32> {
    let mut out = empty(1.3);
    let mut hiss = Hiss(251);
    let mut low = Low::new(350.0);
    for (k, note) in [62.0, 57.0, 50.0].into_iter().enumerate() {
        let f = pitch(note);
        strike(
            &mut out,
            k as f32 * 0.13,
            1.0,
            &[(f, 1.0, 0.14), (2.0 * f, 0.3, 0.06)],
        );
    }
    burst(&mut out, &mut hiss, 0.26, 0.4, (0.004, 0.04), |x, _| {
        low.run(x) * 3.0
    });
    finish(out, 0.4, false)
}

/// Something needs the chief: the second and the sixth of the key,
/// a question left open for the answer to close.
fn chime() -> Vec<f32> {
    let mut out = empty(1.8);
    bell(&mut out, 0.0, pitch(76.0), 1.0);
    bell(&mut out, 0.19, pitch(83.0), 0.7);
    finish(out, 0.4, false)
}

/// A fault: a soft thud, and a low round note that sinks a tone.
fn fault() -> Vec<f32> {
    let mut out = empty(1.3);
    let mut hiss = Hiss(97);
    let mut low = Low::new(400.0);
    strike(
        &mut out,
        0.0,
        0.3,
        &[(90.0, 1.0, 0.05), (210.0, 0.5, 0.04)],
    );
    burst(&mut out, &mut hiss, 0.0, 0.3, (0.002, 0.025), |x, _| {
        low.run(x) * 3.0
    });
    let mut phase = 0.0f32;
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 * DT;
        // From the sixth down to the fifth in its first half second.
        let fallen = (t / 0.5).min(1.0);
        phase += TAU * pitch(59.0 - 2.0 * fallen) * DT;
        let note = phase.sin()
            + 0.4 * (2.0 * phase).sin()
            + 0.12 * (3.0 * phase).sin();
        *s += 0.8 * hit(t, 0.02, 0.4) * note;
    }
    finish(out, 0.36, false)
}

/// The samples of a sound of the world's own events.
pub(super) fn make(sound: Sound) -> Option<Vec<f32>> {
    Some(match sound {
        Sound::HouseRise => house_rise(),
        Sound::HouseSettle => house_settle(),
        Sound::HouseShudder => house_shudder(),
        Sound::HouseFall => house_fall(),
        Sound::Rune => rune(),
        Sound::Embers => embers(),
        Sound::MergeAway => merge_away(),
        Sound::MergeLand => merge_land(),
        Sound::RoadLaid => road_laid(),
        Sound::RoadGlow => road_glow(),
        Sound::Speak => speak(),
        Sound::Think => think(),
        Sound::Muse => muse(),
        Sound::Dawn => dawn(),
        Sound::Dusk => dusk(),
        Sound::Born => born(),
        Sound::Gone => gone(),
        Sound::Restored => restored(),
        Sound::VoteCalled => vote_called(),
        Sound::Ballot => ballot(),
        Sound::Mayor => mayor(),
        Sound::Founded => founded(),
        Sound::Allowed => allowed(),
        Sound::Denied => denied(),
        Sound::Stopped => stopped(),
        Sound::Chime => chime(),
        Sound::Fault => fault(),
        _ => return None,
    })
}
