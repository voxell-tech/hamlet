use std::collections::HashMap;

use super::heard::turn_of_day;
use super::mix::{Heard, admit};
use super::*;
use crate::DayPart;
use crate::play::Play;

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>()
        / samples.len().max(1) as f32)
        .sqrt()
}

#[test]
fn every_sound_is_made_whole() {
    for sound in Sound::ALL {
        let samples = sound.samples();
        let peak = peak(&samples);
        assert!(samples.iter().all(|s| s.is_finite()), "{sound:?}");
        assert!((0.15..=0.7).contains(&peak), "{sound:?} {peak}");
        let rms = rms(&samples);
        assert!(rms > 0.004, "{sound:?} {rms}");
        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        assert!(mean.abs() < 1e-3, "{sound:?} {mean}");
        if !sound.ambient() {
            // It ends in silence, so it does not click.
            assert!(samples.last().unwrap().abs() < 0.01);
            assert!(samples.len() < 2 * RATE as usize);
            // It begins near silence too.
            assert!(samples[0].abs() < 0.05, "{sound:?}");
        }
    }
}

#[test]
fn no_two_sounds_are_the_same_clip() {
    let clips = Sound::ALL.map(|sound| sound.samples());
    for (i, clip) in clips.iter().enumerate() {
        for (k, other) in clips.iter().enumerate().skip(i + 1) {
            assert_ne!(
                clip,
                other,
                "{:?} {:?}",
                Sound::ALL[i],
                Sound::ALL[k]
            );
        }
    }
}

#[test]
fn everything_done_by_hand_has_a_sound_of_its_own() {
    let plays = [
        Play::Select,
        Play::Focus,
        Play::Menu,
        Play::Poke,
        Play::Annoy,
        Play::Pat,
        Play::Lift,
        Play::Drop,
        Play::Wave,
        Play::Cheer,
        Play::Letter,
        Play::Nudge,
        Play::Introduce,
        Play::Gift,
        Play::WickOut,
        Play::WickIn,
        Play::Lever,
    ];
    let sounds = plays.map(Sound::played);
    for (i, sound) in sounds.iter().enumerate() {
        assert_eq!(sound.family(), Family::Hand, "{sound:?}");
        assert!(!sound.everywhere());
        assert!(rms(&sound.samples()) > 0.004, "{sound:?}");
        assert!(!sounds[i + 1..].contains(sound), "{sound:?}");
    }
    // Every sound of the hand answers something done by hand.
    let of_hand = Sound::ALL
        .into_iter()
        .filter(|s| s.family() == Family::Hand)
        .count();
    assert_eq!(of_hand, plays.len());
}

#[test]
fn a_sound_with_a_note_stays_in_the_key() {
    let set = SoundSettings::default();
    for sound in Sound::ALL {
        for turn in 0..40 {
            let speed = sound.speed(&set, turn);
            let moved = 12.0 * speed.log2();
            if sound.steps().is_empty() {
                assert!(moved.abs() < 1.3, "{sound:?}");
                continue;
            }
            // Within a few cents of one of its steps.
            let off = sound
                .steps()
                .iter()
                .map(|step| (moved - step).abs())
                .fold(f32::MAX, f32::min);
            assert!(off < 0.08, "{sound:?} {moved}");
        }
    }
}

#[test]
fn no_sound_is_much_louder_than_its_family() {
    for family in Family::ALL {
        let levels = Sound::ALL
            .into_iter()
            .filter(|s| s.family() == family)
            .map(|s| (s, peak(&s.samples())))
            .collect::<Vec<_>>();
        let mean = levels.iter().map(|l| l.1).sum::<f32>()
            / levels.len() as f32;
        for (sound, level) in levels {
            let over = 20.0 * (level / mean).log10();
            assert!(over < 4.5, "{sound:?} {over} dB");
        }
    }
}

#[test]
fn a_family_takes_only_so_many_and_no_echo_of_itself() {
    let mut set = SoundSettings::default();
    set.limits.hand = Limit { most: 2, gap: 0.5 };
    let cue = |sound| Cue {
        sound,
        at: Vec3::ZERO,
        turn: 0,
    };
    let mut heard = Heard::default();
    let mut playing = HashMap::<Family, usize>::new();
    assert!(admit(&cue(Sound::Pat), &set, &mut heard, &playing, 1.0));
    playing.insert(Family::Hand, 1);
    // The same sound again too soon, then late enough.
    assert!(!admit(
        &cue(Sound::Pat),
        &set,
        &mut heard,
        &playing,
        1.2
    ));
    assert!(admit(&cue(Sound::Pat), &set, &mut heard, &playing, 1.6));
    playing.insert(Family::Hand, 2);
    assert!(!admit(
        &cue(Sound::Poke),
        &set,
        &mut heard,
        &playing,
        3.0
    ));
    // Another family is not held back by it.
    assert!(admit(&cue(Sound::Tab), &set, &mut heard, &playing, 3.0));
    // A second alert within the gap is left out.
    assert!(admit(
        &cue(Sound::Chime),
        &set,
        &mut heard,
        &playing,
        4.0
    ));
    assert!(!admit(
        &cue(Sound::Fault),
        &set,
        &mut heard,
        &playing,
        6.0
    ));
    assert!(admit(
        &cue(Sound::Fault),
        &set,
        &mut heard,
        &playing,
        9.5
    ));
}

#[test]
fn the_day_turning_is_heard_once_each_way() {
    let mut app = App::new();
    app.add_plugins(bevy::state::app::StatesPlugin)
        .init_state::<DayPart>()
        .add_message::<Cue>()
        .add_systems(Update, turn_of_day);
    let mut heard = Vec::new();
    for part in [
        DayPart::Day,
        DayPart::Dusk,
        DayPart::Night,
        DayPart::Night,
        DayPart::Dawn,
        DayPart::Day,
    ] {
        app.world_mut()
            .resource_mut::<NextState<DayPart>>()
            .set(part);
        app.update();
        let cues = app.world().resource::<Messages<Cue>>();
        heard.extend(
            cues.iter_current_update_messages().map(|cue| cue.sound),
        );
    }
    assert_eq!(heard, [Sound::Dusk, Sound::Dawn]);
}

#[test]
fn a_loop_has_no_seam() {
    for sound in Sound::ALL.into_iter().filter(|s| s.ambient()) {
        let samples = sound.samples();
        let jump = |a: f32, b: f32| (a - b).abs();
        // The step over the seam is no larger than the steps
        // inside the loop.
        let largest = samples
            .windows(2)
            .map(|w| jump(w[0], w[1]))
            .fold(0.0f32, f32::max);
        let seam = jump(samples[samples.len() - 1], samples[0]);
        assert!(seam <= largest, "{sound:?} {seam} {largest}");
    }
}
