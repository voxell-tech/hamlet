//! Every sound of the realm, and the family each belongs to. The
//! panels' own sounds are the engine's.

use crate::sound::{Limit, SoundSettings};

/// A kind of sound, with a volume and a limit of its own.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Family {
    /// Birds, crickets and wind.
    Ambience,
    Steps,
    /// The sounds of a robot's chores.
    Work,
    /// Doors and letters.
    Doors,
    /// The calls for the chief.
    Alerts,
    /// The chief's own hand on the world.
    Hand,
    /// Houses, runes, embers, merges, roads and the turn of the
    /// day.
    Magic,
    /// Balloons and the thinking light.
    Talk,
    /// The fates of agents and hamlets.
    Lives,
    /// The chief's answers.
    Answers,
}

impl Family {
    /// Its volume, before the engine's.
    pub fn volume(self, set: &SoundSettings) -> f32 {
        match self {
            Self::Ambience => set.ambience,
            Self::Steps => set.steps,
            Self::Work => set.work,
            Self::Doors => set.doors,
            Self::Alerts => set.alerts,
            Self::Hand => set.hand,
            Self::Magic => set.magic,
            Self::Talk => set.talk,
            Self::Lives => set.lives,
            Self::Answers => set.answers,
        }
    }

    /// Number of it that play at once and the least time before one
    /// repeats. The ambience has no limit: its loops are started
    /// once.
    pub fn limit(self, set: &SoundSettings) -> Limit {
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
            Self::Hand => limits.hand,
            Self::Magic => limits.magic,
            Self::Talk => limits.talk,
            Self::Lives => limits.lives,
            Self::Answers => limits.answers,
        }
    }

    /// True when it is of the world's own doing, and so counts toward
    /// the most sounds the world plays at once.
    pub fn worldly(self) -> bool {
        matches!(
            self,
            Self::Steps
                | Self::Work
                | Self::Doors
                | Self::Magic
                | Self::Talk
                | Self::Lives
        )
    }
}

/// One of the realm's sounds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sound {
    /// A foot on grass or on a yard's paving.
    Step,
    /// A foot on a road.
    RoadStep,
    Hammer,
    /// One turn of an engine's wheel.
    Wheel,
    /// A puff of an engine's steam.
    Steam,
    Quill,
    /// A kite's cloth in the wind.
    Kite,
    Door,
    /// Something needs the chief.
    Chime,
    /// A low note for a fault.
    Fault,
    /// A letter or a page changing hands.
    Letter,
    Birds,
    Crickets,
    Wind,
    /// A thing in the world chosen, and the view brought to one.
    Select,
    Focus,
    /// A menu opened over a thing in the world.
    Menu,
    /// A robot poked, and poked once too often.
    Poke,
    Annoy,
    Pat,
    /// A robot or a place picked up, and set down.
    Lift,
    Drop,
    Wave,
    Cheer,
    /// Something handed to a robot to read.
    Hand,
    /// A thinking light nudged.
    Nudge,
    Introduce,
    /// A skill given.
    Gift,
    /// A chief's mascot going out by Wickstep, and lit again.
    WickOut,
    WickIn,
    /// A hamlet's clockwork switched.
    Clockwork,
    /// A house standing up, and coming to rest.
    HouseRise,
    HouseSettle,
    /// A house about to fall, and fallen.
    HouseShudder,
    HouseFall,
    /// A circle of runes opening under a house that rises.
    Rune,
    Embers,
    /// A merged house's motes setting out, and arriving.
    MergeAway,
    MergeLand,
    RoadLaid,
    /// A road lighting as a message sets out along it.
    RoadGlow,
    /// A balloon for speech, and one for a thought.
    Speak,
    Think,
    /// The light of an agent that has begun to think.
    Muse,
    Dawn,
    Dusk,
    /// An agent made, removed and restored.
    Born,
    Gone,
    Restored,
    VoteCalled,
    Ballot,
    /// A hamlet has another mayor.
    Mayor,
    Founded,
    /// The chief said yes, and no.
    Allowed,
    Denied,
    /// The chief stopped an agent, or everything.
    Stopped,
}

impl Sound {
    pub const ALL: [Self; 56] = [
        Self::Step,
        Self::RoadStep,
        Self::Hammer,
        Self::Wheel,
        Self::Steam,
        Self::Quill,
        Self::Kite,
        Self::Door,
        Self::Chime,
        Self::Fault,
        Self::Letter,
        Self::Birds,
        Self::Crickets,
        Self::Wind,
        Self::Select,
        Self::Focus,
        Self::Menu,
        Self::Poke,
        Self::Annoy,
        Self::Pat,
        Self::Lift,
        Self::Drop,
        Self::Wave,
        Self::Cheer,
        Self::Hand,
        Self::Nudge,
        Self::Introduce,
        Self::Gift,
        Self::WickOut,
        Self::WickIn,
        Self::Clockwork,
        Self::HouseRise,
        Self::HouseSettle,
        Self::HouseShudder,
        Self::HouseFall,
        Self::Rune,
        Self::Embers,
        Self::MergeAway,
        Self::MergeLand,
        Self::RoadLaid,
        Self::RoadGlow,
        Self::Speak,
        Self::Think,
        Self::Muse,
        Self::Dawn,
        Self::Dusk,
        Self::Born,
        Self::Gone,
        Self::Restored,
        Self::VoteCalled,
        Self::Ballot,
        Self::Mayor,
        Self::Founded,
        Self::Allowed,
        Self::Denied,
        Self::Stopped,
    ];

    pub fn family(self) -> Family {
        match self {
            Self::Birds | Self::Crickets | Self::Wind => {
                Family::Ambience
            }
            Self::Step | Self::RoadStep => Family::Steps,
            Self::Hammer
            | Self::Wheel
            | Self::Steam
            | Self::Quill
            | Self::Kite => Family::Work,
            Self::Door | Self::Letter => Family::Doors,
            Self::Chime | Self::Fault => Family::Alerts,
            Self::Select
            | Self::Focus
            | Self::Menu
            | Self::Poke
            | Self::Annoy
            | Self::Pat
            | Self::Lift
            | Self::Drop
            | Self::Wave
            | Self::Cheer
            | Self::Hand
            | Self::Nudge
            | Self::Introduce
            | Self::Gift
            | Self::WickOut
            | Self::WickIn
            | Self::Clockwork => Family::Hand,
            Self::HouseRise
            | Self::HouseSettle
            | Self::HouseShudder
            | Self::HouseFall
            | Self::Rune
            | Self::Embers
            | Self::MergeAway
            | Self::MergeLand
            | Self::RoadLaid
            | Self::RoadGlow
            | Self::Dawn
            | Self::Dusk => Family::Magic,
            Self::Speak | Self::Think | Self::Muse => Family::Talk,
            Self::Born
            | Self::Gone
            | Self::Restored
            | Self::VoteCalled
            | Self::Ballot
            | Self::Mayor
            | Self::Founded => Family::Lives,
            Self::Allowed | Self::Denied | Self::Stopped => {
                Family::Answers
            }
        }
    }

    /// True when it is a loop of the ambience.
    pub fn ambient(self) -> bool {
        self.family() == Family::Ambience
    }

    /// True when it asks for the chief's attention.
    pub fn alert(self) -> bool {
        self.family() == Family::Alerts
    }

    /// True when it is heard the same wherever the camera is: the
    /// chief's answers, the turn of the day, and what becomes of a
    /// whole hamlet.
    pub fn everywhere(self) -> bool {
        self.family() == Family::Answers
            || matches!(
                self,
                Self::Dawn | Self::Dusk | Self::Founded | Self::Mayor
            )
    }

    /// The semitones it may be moved by from one playing to the
    /// next and stay in the key. Empty for a sound with no note,
    /// whose pitch strays freely.
    pub fn steps(self) -> &'static [f32] {
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
            | Self::Birds
            | Self::Crickets
            | Self::Wind
            | Self::Menu
            | Self::Speak
            | Self::Think
            | Self::HouseRise
            | Self::HouseShudder
            | Self::HouseFall
            | Self::Embers
            | Self::Ballot
            | Self::Clockwork => &[],
            // Each of these is a note or two of the pentatonic
            // scale, and its moves put it on other notes of that
            // scale: from the keynote or the fifth,
            Self::Poke => &[0.0, 2.0, -3.0],
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

    /// Speed it is played at the `turn`th time: on another note of
    /// the key if it has a note, and a little off either way.
    pub fn speed(self, set: &SoundSettings, turn: u32) -> f32 {
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
