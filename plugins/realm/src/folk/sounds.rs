//! The sounds the robots and the chief make, each raised as a
//! [`FolkCue`] where it is made.

use glam::Vec3;
use hamlet_engine_bevy::prelude::*;

/// A sound of the robots, of the chief's hand on them, or of the
/// chief's mascot.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FolkSound {
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
    /// A letter or a page changing hands.
    Letter,
    /// A robot poked, and poked once too often.
    Poke,
    Annoy,
    Pat,
    /// A robot picked up, and set down.
    Lift,
    Drop,
    Wave,
    Cheer,
    Introduce,
    /// A chief's mascot going out by Wickstep, and lit again.
    WickOut,
    WickIn,
}

/// A sound that has just been made somewhere in the realm.
#[derive(Message, Clone, Copy, Debug)]
pub struct FolkCue {
    pub sound: FolkSound,
    pub at: Vec3,
    /// A number that differs from one cue to the next, to vary the
    /// pitch.
    pub turn: u32,
}
