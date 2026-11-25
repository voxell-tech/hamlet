//! What a [`Sound`] and its [`Family`] need of the realm: settings,
//! samples and the things done by hand.

use hamlet_core::sound::{Family, Sound};

use super::{Limit, SoundSettings, synth};
use crate::play::Play;

/// How loud a family is, and how much of it plays at once.
pub trait Mix {
    /// Its volume, before the master volume.
    fn volume(self, set: &SoundSettings) -> f32;

    /// Number of it that play at once and the least time before one
    /// repeats. The ambience has no limit: its loops are started
    /// once.
    fn limit(self, set: &SoundSettings) -> Limit;
}

impl Mix for Family {
    fn volume(self, set: &SoundSettings) -> f32 {
        match self {
            Self::Ambience => set.ambience,
            Self::Steps => set.steps,
            Self::Work => set.work,
            Self::Doors => set.doors,
            Self::Alerts => set.alerts,
            Self::Interface => set.interface,
            Self::Hand => set.hand,
            Self::Magic => set.magic,
            Self::Talk => set.talk,
            Self::Lives => set.lives,
            Self::Answers => set.answers,
        }
    }

    fn limit(self, set: &SoundSettings) -> Limit {
        let limits = &set.limits;
        match self {
            Self::Ambience => Limit {
                most: usize::MAX,
                gap: 0.0,
            },
            Self::Steps => limits.steps,
            Self::Work => limits.work,
            Self::Doors => limits.doors,
            Self::Alerts => limits.alerts,
            Self::Interface => limits.interface,
            Self::Hand => limits.hand,
            Self::Magic => limits.magic,
            Self::Talk => limits.talk,
            Self::Lives => limits.lives,
            Self::Answers => limits.answers,
        }
    }
}

/// The sound of a thing done by hand.
pub(super) fn played(play: Play) -> Sound {
    match play {
        Play::Select => Sound::Select,
        Play::Focus => Sound::Focus,
        Play::Menu => Sound::Menu,
        Play::Poke => Sound::Poke,
        Play::Annoy => Sound::Annoy,
        Play::Pat => Sound::Pat,
        Play::Lift => Sound::Lift,
        Play::Drop => Sound::Drop,
        Play::Wave => Sound::Wave,
        Play::Cheer => Sound::Cheer,
        Play::Nudge => Sound::Nudge,
        Play::Introduce => Sound::Introduce,
        Play::WickOut => Sound::WickOut,
        Play::WickIn => Sound::WickIn,
        Play::Clockwork => Sound::Clockwork,
    }
}

/// What a sound is made of and how it is played.
pub trait Playback {
    /// The semitones it may be moved by from one playing to the
    /// next and stay in the key. Empty for a sound with no note,
    /// whose pitch strays freely.
    fn steps(self) -> &'static [f32];

    /// The samples of the sound, at `RATE` a second.
    fn samples(self) -> Vec<f32>;

    /// Speed it is played at the `turn`th time: on another note of
    /// the key if it has a note, and a little off either way.
    fn speed(self, set: &SoundSettings, turn: u32) -> f32;
}

impl Playback for Sound {
    fn steps(self) -> &'static [f32] {
        match self {
            Self::Step
            | Self::RoadStep
            | Self::Hammer
            | Self::Wheel
            | Self::Steam
            | Self::Quill
            | Self::Kite
            | Self::Door
            | Self::Letter
            | Self::Wind
            | Self::Tap
            | Self::Menu
            | Self::Speak
            | Self::Think
            | Self::HouseRise
            | Self::HouseShudder
            | Self::HouseFall
            | Self::Embers
            | Self::PopOpen
            | Self::Grab
            | Self::Dock
            | Self::Clockwork => &[],
            // Each of these is a note or two of the pentatonic
            // scale, and its moves put it on other notes of that
            // scale: from the keynote or the fifth,
            Self::Tick | Self::Tab | Self::Poke => &[0.0, 2.0, -3.0],
            Self::Select => &[0.0, 2.0, -3.0, 5.0],
            // from the sixth or the third,
            Self::Nudge => &[0.0, -2.0, 3.0],
            // and for two notes a third or a fourth apart.
            Self::Wave | Self::RoadGlow => &[0.0, 5.0],
            Self::Muse => &[0.0, 5.0, -5.0],
            // The rest are figures of several notes, and stay put.
            _ => &[0.0],
        }
    }

    fn samples(self) -> Vec<f32> {
        synth::make(self)
    }

    fn speed(self, set: &SoundSettings, turn: u32) -> f32 {
        let mixed = turn.wrapping_mul(2_654_435_761);
        let stray = (mixed >> 16) as f32 / 65_535.0 * 2.0 - 1.0;
        let steps = self.steps();
        if steps.is_empty() {
            return 1.0 + stray * set.vary;
        }
        let step = steps[(mixed >> 8) as usize % steps.len()];
        2f32.powf(step / 12.0) * (1.0 + stray * set.vary_tuned)
    }
}
