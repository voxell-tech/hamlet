//! Every sound there is, and the family each belongs to.

use super::{Limit, SoundSettings, synth};
use crate::play::Play;

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
    /// The panels.
    Interface,
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
    #[cfg(test)]
    pub const ALL: [Self; 11] = [
        Self::Ambience,
        Self::Steps,
        Self::Work,
        Self::Doors,
        Self::Alerts,
        Self::Interface,
        Self::Hand,
        Self::Magic,
        Self::Talk,
        Self::Lives,
        Self::Answers,
    ];

    /// Its volume, before the master volume.
    pub fn volume(self, set: &SoundSettings) -> f32 {
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

    /// Number of it that play at once and the least time before one
    /// repeats. The ambience has no limit: its loops are started
    /// once.
    pub(super) fn limit(self, set: &SoundSettings) -> Limit {
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

    /// True when it is of the world's own doing, and so counts toward
    /// the most sounds the world plays at once.
    pub(super) fn worldly(self) -> bool {
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

/// One of the world's sounds.
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
    /// The pointer coming onto a button.
    Tick,
    /// A button going down.
    Tap,
    /// The action a screen is for, done.
    Confirm,
    /// Something refused or stopped.
    Deny,
    /// A thing in the world chosen, and the view brought to one.
    Select,
    Focus,
    /// A menu opened over a thing in the world.
    Menu,
    /// A robot poked, and poked once too often.
    Poke,
    Annoy,
    Pat,
    /// A robot picked up, and set down.
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
    /// A menu or a popup opening, and closing.
    PopOpen,
    PopClose,
    /// A tab brought to the front.
    Tab,
    /// A fold opened or shut.
    Fold,
    /// A message sent from the command line.
    Send,
    /// The card of a thing the pointer rests on.
    Card,
    /// A panel taken hold of, and set in its place.
    Grab,
    Dock,
    /// One of the window's own buttons.
    Sash,
    Error,
    /// The chief said yes, and no.
    Allowed,
    Denied,
    /// The chief stopped an agent, or everything.
    Stopped,
}

impl Sound {
    pub const ALL: [Self; 70] = [
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
        Self::Tick,
        Self::Tap,
        Self::Confirm,
        Self::Deny,
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
        Self::PopOpen,
        Self::PopClose,
        Self::Tab,
        Self::Fold,
        Self::Send,
        Self::Card,
        Self::Grab,
        Self::Dock,
        Self::Sash,
        Self::Error,
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
            Self::Tick
            | Self::Tap
            | Self::Confirm
            | Self::Deny
            | Self::PopOpen
            | Self::PopClose
            | Self::Tab
            | Self::Fold
            | Self::Send
            | Self::Card
            | Self::Grab
            | Self::Dock
            | Self::Sash
            | Self::Error => Family::Interface,
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
    pub(crate) fn alert(self) -> bool {
        self.family() == Family::Alerts
    }

    /// True when it is heard the same wherever the camera is: the
    /// panels, the chief's answers, the turn of the day, and what
    /// becomes of a whole hamlet.
    pub fn everywhere(self) -> bool {
        matches!(self.family(), Family::Interface | Family::Answers)
            || matches!(
                self,
                Self::Dawn | Self::Dusk | Self::Founded | Self::Mayor
            )
    }

    /// The sound of a thing done by hand.
    pub fn played(play: Play) -> Self {
        match play {
            Play::Select => Self::Select,
            Play::Focus => Self::Focus,
            Play::Menu => Self::Menu,
            Play::Poke => Self::Poke,
            Play::Annoy => Self::Annoy,
            Play::Pat => Self::Pat,
            Play::Lift => Self::Lift,
            Play::Drop => Self::Drop,
            Play::Wave => Self::Wave,
            Play::Cheer => Self::Cheer,
            Play::Letter => Self::Hand,
            Play::Nudge => Self::Nudge,
            Play::Introduce => Self::Introduce,
            Play::Gift => Self::Gift,
            Play::WickOut => Self::WickOut,
            Play::WickIn => Self::WickIn,
            Play::Clockwork => Self::Clockwork,
        }
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
            | Self::Tap
            | Self::Menu
            | Self::Speak
            | Self::Think
            | Self::HouseRise
            | Self::HouseShudder
            | Self::HouseFall
            | Self::Embers
            | Self::Ballot
            | Self::PopOpen
            | Self::PopClose
            | Self::Fold
            | Self::Card
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

    pub fn name(self) -> &'static str {
        match self {
            Self::Step => "step",
            Self::RoadStep => "road-step",
            Self::Hammer => "hammer",
            Self::Wheel => "wheel",
            Self::Steam => "steam",
            Self::Quill => "quill",
            Self::Kite => "kite",
            Self::Door => "door",
            Self::Chime => "chime",
            Self::Fault => "fault",
            Self::Letter => "letter",
            Self::Birds => "birds",
            Self::Crickets => "crickets",
            Self::Wind => "wind",
            Self::Tick => "tick",
            Self::Tap => "tap",
            Self::Confirm => "confirm",
            Self::Deny => "deny",
            Self::Select => "select",
            Self::Focus => "focus",
            Self::Menu => "menu",
            Self::Poke => "poke",
            Self::Annoy => "annoy",
            Self::Pat => "pat",
            Self::Lift => "lift",
            Self::Drop => "drop",
            Self::Wave => "wave",
            Self::Cheer => "cheer",
            Self::Hand => "hand",
            Self::Nudge => "nudge",
            Self::Introduce => "introduce",
            Self::Gift => "gift",
            Self::WickOut => "wick-out",
            Self::WickIn => "wick-in",
            Self::Clockwork => "clockwork",
            Self::HouseRise => "house-rise",
            Self::HouseSettle => "house-settle",
            Self::HouseShudder => "house-shudder",
            Self::HouseFall => "house-fall",
            Self::Rune => "rune",
            Self::Embers => "embers",
            Self::MergeAway => "merge-away",
            Self::MergeLand => "merge-land",
            Self::RoadLaid => "road-laid",
            Self::RoadGlow => "road-glow",
            Self::Speak => "speak",
            Self::Think => "think",
            Self::Muse => "muse",
            Self::Dawn => "dawn",
            Self::Dusk => "dusk",
            Self::Born => "born",
            Self::Gone => "gone",
            Self::Restored => "restored",
            Self::VoteCalled => "vote-called",
            Self::Ballot => "ballot",
            Self::Mayor => "mayor",
            Self::Founded => "founded",
            Self::PopOpen => "pop-open",
            Self::PopClose => "pop-close",
            Self::Tab => "tab",
            Self::Fold => "fold",
            Self::Send => "send",
            Self::Card => "card",
            Self::Grab => "grab",
            Self::Dock => "dock",
            Self::Sash => "sash",
            Self::Error => "error",
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::Stopped => "stopped",
        }
    }

    /// The samples of the sound, at `RATE` a second.
    pub fn samples(self) -> Vec<f32> {
        synth::make(self)
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
