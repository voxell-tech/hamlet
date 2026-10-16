//! Writing a sound cue from a panel.

use bevy::ecs::message::MessageWriter;
use bevy::math::Vec3;
use hamlet_world::sound::{Cue, Sound};

/// A cue for `sound`, heard from nowhere in particular.
pub fn of(sound: Sound) -> Cue {
    Cue {
        sound,
        at: Vec3::ZERO,
        turn: 0,
    }
}

/// Writes a cue with a number of its own, so that it is not played
/// twice alike.
pub fn write(
    cues: &mut MessageWriter<Cue>,
    turn: &mut u32,
    sound: Sound,
) {
    *turn = turn.wrapping_add(1);
    cues.write(Cue {
        turn: *turn,
        ..of(sound)
    });
}
