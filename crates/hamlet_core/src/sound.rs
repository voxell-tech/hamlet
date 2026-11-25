//! The words of the world's sound: every [`Sound`], the [`Family`]
//! each belongs to, and the [`Cue`] that asks for one to be played.
//! How a sound is made and heard is `hamlet_realm`'s.

use bevy::app::App;
use bevy::ecs::message::Message;
use bevy::math::Vec3;

pub fn plugin(app: &mut App) {
    app.add_message::<Cue>();
}

/// A sound that has just been made somewhere in the world.
#[derive(Message, Clone, Copy, Debug)]
pub struct Cue {
    pub sound: Sound,
    pub at: Vec3,
    /// A number that differs from one cue to the next, to vary the
    /// pitch.
    pub turn: u32,
}

/// A kind of sound, with a volume and a limit of its own.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Family {
    /// The environment: wind.
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
    /// True when it is of the world's own doing, and so counts toward
    /// the most sounds the world plays at once.
    pub fn is_worldly(self) -> bool {
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
    /// A hamlet has another lead.
    Lead,
    Founded,
    /// A menu or a popup opening.
    PopOpen,
    /// A tab brought to the front.
    Tab,
    /// A message sent from the command line.
    Send,
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
    pub const ALL: [Self; 62] = [
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
        Self::Lead,
        Self::Founded,
        Self::PopOpen,
        Self::Tab,
        Self::Send,
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
            Self::Wind => Family::Ambience,
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
            | Self::Tab
            | Self::Send
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
            | Self::Lead
            | Self::Founded => Family::Lives,
            Self::Allowed | Self::Denied | Self::Stopped => {
                Family::Answers
            }
        }
    }

    /// True when it is a loop of the ambience.
    pub fn is_ambient(self) -> bool {
        self.family() == Family::Ambience
    }

    /// True when it asks for the chief's attention.
    pub fn is_alert(self) -> bool {
        self.family() == Family::Alerts
    }

    /// True when it is heard the same wherever the camera is: the
    /// panels, the chief's answers, the turn of the day, and what
    /// becomes of a whole hamlet.
    pub fn is_heard_everywhere(self) -> bool {
        matches!(self.family(), Family::Interface | Family::Answers)
            || matches!(
                self,
                Self::Dawn | Self::Dusk | Self::Founded | Self::Lead
            )
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
            Self::Lead => "lead",
            Self::Founded => "founded",
            Self::PopOpen => "pop-open",
            Self::Tab => "tab",
            Self::Send => "send",
            Self::Grab => "grab",
            Self::Dock => "dock",
            Self::Sash => "sash",
            Self::Error => "error",
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::Stopped => "stopped",
        }
    }
}
