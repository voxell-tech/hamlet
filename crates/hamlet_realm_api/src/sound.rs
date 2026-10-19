//! The panels' own sounds, which the realm plays, and the chief's
//! choice of what is heard.

use bevy::ecs::message::{Message, MessageWriter};
use bevy::ecs::resource::Resource;

/// A sound of the panels.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PanelSound {
    /// The pointer coming onto a button.
    Tick,
    /// A button going down.
    Tap,
    /// The action a screen is for, done.
    Confirm,
    /// Something refused or stopped.
    Deny,
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
}

/// A sound the panels have just made.
#[derive(Message, Clone, Copy, Debug)]
pub struct PanelCue {
    pub sound: PanelSound,
    /// A number that differs from one cue to the next, to vary the
    /// pitch.
    pub turn: u32,
}

/// Writes a cue of `sound` with a number of its own, so that it is
/// not played twice alike.
pub fn write(
    cues: &mut MessageWriter<PanelCue>,
    turn: &mut u32,
    sound: PanelSound,
) {
    *turn = turn.wrapping_add(1);
    cues.write(PanelCue { sound, turn: *turn });
}

/// The chief's choice of the effects and the music, and the level of
/// each, 0 to 1.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct SoundChoice {
    pub muted: bool,
    pub music_on: bool,
    pub sound_level: f32,
    pub music_level: f32,
}
