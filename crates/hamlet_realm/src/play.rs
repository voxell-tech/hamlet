//! The deeds of a chief's hand on the world, told as they happen.

use bevy::prelude::*;

pub fn plugin(app: &mut App) {
    app.add_message::<Played>();
}

/// One thing done by hand.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Play {
    /// A thing was selected, or brought to the middle of the view.
    Select,
    Focus,
    /// A menu was opened over the world.
    Menu,
    /// A robot was poked, and poked once too often.
    Poke,
    Annoy,
    /// A robot was patted.
    Pat,
    /// A robot or a place was picked up, and set down again.
    Lift,
    Drop,
    /// A robot waved at the pointer.
    Wave,
    /// A robot was cheered for its work.
    Cheer,
    /// A robot was handed something to read.
    Letter,
    /// A thinking agent's light was nudged.
    Nudge,
    /// Two robots were introduced, for a road between them.
    Introduce,
    /// An agent was given a skill.
    Gift,
    /// A chief's mascot went out by Wickstep, and was lit again
    /// where it was going.
    WickOut,
    WickIn,
    /// A hamlet's clockwork was switched.
    Clockwork,
}

/// A thing done by hand, and where in the world.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Played {
    pub what: Play,
    pub at: Vec3,
}
