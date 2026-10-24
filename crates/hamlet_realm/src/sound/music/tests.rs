use super::*;

fn mood(part: DayPart, busy: f32) -> Mood {
    Mood {
        part,
        busy,
        rest: 0.0,
    }
}

/// The pitch classes of D major with the lydian fourth.
fn is_in_key(note: f32) -> bool {
    [1.0, 2.0, 4.0, 6.0, 7.0, 8.0, 9.0, 11.0]
        .contains(&note.rem_euclid(12.0))
}

#[test]
fn every_note_is_in_the_key() {
    for part in
        [DayPart::Day, DayPart::Night, DayPart::Dawn, DayPart::Dusk]
    {
        let mut composer = Composer::new(11);
        for k in 0..200 {
            let mood = mood(part, (k % 5) as f32 / 4.0);
            let breath = composer.breathe(&mood);
            assert!(breath.pads.iter().all(|p| is_in_key(p.0)));
            assert!(breath.notes.iter().all(|n| is_in_key(n.note)));
            for moment in Moment::ALL {
                let phrase = composer.answer(moment, &mood);
                assert!(
                    phrase.notes.iter().all(|n| is_in_key(n.note)),
                    "{moment:?}"
                );
            }
        }
    }
}

#[test]
fn the_music_is_sparser_at_night_and_at_rest() {
    let per_minute = |part, busy| {
        let mut composer = Composer::new(5);
        let (mut notes, mut seconds) = (0, 0.0);
        for _ in 0..300 {
            let breath = composer.breathe(&mood(part, busy));
            notes += breath
                .notes
                .iter()
                .filter(|n| n.voice != Voice::Bass)
                .count();
            seconds += breath.stride;
        }
        notes as f32 * 60.0 / seconds
    };
    let (day, night) = (DayPart::Day, DayPart::Night);
    assert!(per_minute(day, 1.0) > 1.5 * per_minute(day, 0.0));
    assert!(per_minute(day, 0.0) > 1.3 * per_minute(night, 0.0));
    // Never busy: fewer than a note a second at its fullest.
    assert!(per_minute(day, 1.0) < 60.0);
    assert!(per_minute(night, 0.0) > 1.0);
}

#[test]
fn a_breath_fits_its_tape_and_overlaps_the_next() {
    let mut composer = Composer::new(3);
    for k in 0..300 {
        let part = if k % 2 == 0 {
            DayPart::Day
        } else {
            DayPart::Night
        };
        let breath = composer.breathe(&mood(part, 0.5));
        let (_, hold, fall) = breath.shape;
        assert!(hold + fall <= breath.long);
        assert!(breath.stride > 4.0 && breath.stride < 42.0);
        for n in &breath.notes {
            assert!(n.at > 0.0 && n.at < hold, "{n:?}");
            assert!(n.gain > 0.0 && n.gain < 0.3);
        }
    }
}

#[test]
fn a_breath_begins_and_ends_in_silence_and_is_never_loud() {
    let mut composer = Composer::new(21);
    let mood = mood(DayPart::Day, 1.0);
    let breath = composer.breathe(&mood);
    let phrase = composer.answer(Moment::Found, &mood);
    for (what, most) in [(breath, 0.4), (phrase, 0.3)] {
        let samples = render(&what);
        assert!(samples.iter().all(|s| s.is_finite()));
        assert_eq!(samples.len() % 2, 0);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.01 && peak < most, "{peak}");
        let edge =
            |s: &[f32]| s.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(edge(&samples[..32]) < 1e-3);
        assert!(edge(&samples[samples.len() - 32..]) < 1e-3);
    }
}
