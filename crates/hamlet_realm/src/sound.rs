//! The world's sound (docs/sound.md): clips made in code, played
//! where a [`Cue`] says and heard from near the ground the camera
//! looks at, with a quiet music under them.

mod clip;
mod heard;
mod kinds;
mod mix;
pub mod music;
mod synth;
#[cfg(test)]
mod tests;
pub mod voices;

use bevy::audio::{AddAudioSource, AudioPlugin};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

pub use self::clip::{Clip, Playing};
pub use self::kinds::{Family, Sound};
pub use self::mix::Ears;
pub use self::synth::RATE;
use crate::{Step, WorldCamera, WorldSystems};

/// Sound for a world. Does nothing where the app has no audio.
pub fn plugin(app: &mut App) {
    app.init_resource::<SoundSettings>()
        .add_message::<Cue>()
        .add_observer(heard::hear_answer)
        .add_observer(heard::hear_veto)
        .configure_sets(
            Update,
            Cues.run_if(in_state(Hearing::On)).after(Step::Robots),
        )
        .add_systems(
            Update,
            (heard::hear_hand, heard::turn_of_day).before(Cues),
        );
    if !app.is_plugin_added::<AudioPlugin>() {
        return;
    }
    app.add_audio_source::<Clip>()
        .init_state::<Hearing>()
        .init_resource::<mix::Heard>()
        .add_systems(Startup, mix::spawn_ears)
        .add_systems(Update, listen.before(WorldSystems))
        .add_systems(OnEnter(Hearing::On), clip::make_clips)
        .add_systems(
            Update,
            (
                clip::bank_clips
                    .run_if(resource_exists::<clip::Making>),
                (
                    mix::place_ears,
                    mix::start_ambience,
                    mix::mix_ambience,
                    mix::play,
                )
                    .chain()
                    .run_if(resource_exists::<clip::Bank>),
            )
                .chain()
                .run_if(in_state(Hearing::On))
                .after(Cues),
        )
        .add_plugins(music::band::plugin);
}

/// The share of one kind of sound heard at a time.
#[derive(Clone, Copy, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limit {
    /// The most of the kind playing at once.
    pub most: usize,
    /// Seconds before the same sound may play again.
    pub gap: f32,
}

/// The [`Limit`] of each kind of sound.
#[derive(Clone, Copy, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub steps: Limit,
    pub work: Limit,
    pub doors: Limit,
    pub alerts: Limit,
    pub interface: Limit,
    pub hand: Limit,
    pub magic: Limit,
    pub talk: Limit,
    pub lives: Limit,
    pub answers: Limit,
}

/// The world's sound.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoundSettings {
    /// True when the effects and the ambience are off.
    pub muted: bool,
    /// True when the music plays. It does not follow `muted`.
    pub music_on: bool,
    /// The master volume, 0 to 1, of the effects and the music.
    pub volume: f32,
    /// The chief's own level for the effects, 0 to 1.
    pub sound_level: f32,
    /// The chief's own level for the music, 0 to 1.
    pub music_level: f32,
    /// The volume of each kind of sound, before the master volume:
    /// birds, crickets and wind; footsteps; hammer, wheel, steam,
    /// quill and kite; doors and letters; the chime and the fault
    /// note.
    pub ambience: f32,
    /// The birds' share of the ambience.
    pub birds: f32,
    pub steps: f32,
    pub work: f32,
    pub doors: f32,
    pub alerts: f32,
    /// The volume of the panels' own sounds.
    pub interface: f32,
    /// The volume of the chief's own hand on the world.
    pub hand: f32,
    /// Houses rising and falling, runes, embers, merges, roads and
    /// the turn of the day.
    pub magic: f32,
    /// Balloons and the thinking light.
    pub talk: f32,
    /// Agents made, removed and restored, votes, mayors and new
    /// hamlets.
    pub lives: f32,
    /// The chief's answers, and a stop.
    pub answers: f32,
    /// The volume of the music.
    pub music: f32,
    pub limits: Limits,
    /// Distance from the ears within which a sound is at its full
    /// volume. Beyond it the sound falls with the square of the
    /// distance.
    pub reach: f32,
    /// Distance from the ears beyond which a sound is not played.
    pub earshot: f32,
    /// The furthest from the ears a sound of the chief's own hand
    /// is placed, so that it is always heard.
    pub hand_far: f32,
    /// The place of the ears between the ground the camera looks at,
    /// 0, and the camera, 1.
    pub listener: f32,
    /// The furthest the ears are put from the camera, for a camera
    /// that looks over the ground and not down at it.
    pub furthest: f32,
    /// Distance between the ears.
    pub ears: f32,
    /// The most sounds of the world playing at once, the ambience
    /// aside.
    pub voices: usize,
    /// The share a sound's pitch may stray from one playing to the
    /// next: of a sound with no note, and of one with a note.
    pub vary: f32,
    pub vary_tuned: f32,
    /// The share a sound's volume may stray from one playing to the
    /// next.
    pub sway: f32,
    /// Seconds within which a second chime or fault note is left
    /// out.
    pub alert_gap: f32,
    /// True when the chime and the fault note are as loud wherever
    /// the camera is.
    pub alerts_everywhere: bool,
    /// Seconds the ambience takes to change with the hour.
    pub fade: f32,
    /// Height of the ears below which birds and crickets are at
    /// their loudest.
    pub close: f32,
    /// Seconds the music takes to come in and to go out.
    pub music_fade: f32,
    /// The share of its volume the music keeps at night.
    pub music_night: f32,
    /// The agents at work for which the music is at its most
    /// active.
    pub busy_full: f32,
    /// Seconds of rest added between two breaths of the music.
    /// More makes it sparser.
    pub music_rest: f32,
    /// Seconds before the music answers another moment, and the
    /// seconds it waits before it answers one.
    pub phrase_gap: f32,
    pub phrase_after: f32,
}

impl SoundSettings {
    /// True when the effects can be heard.
    pub fn effects(&self) -> bool {
        !self.muted && self.volume > 0.0
    }

    /// True when the music can be heard.
    pub fn score(&self) -> bool {
        self.music_on && self.volume > 0.0 && self.music > 0.0
    }
}

/// The hearing of the world's effects: on while they are not muted
/// and the world is being drawn.
#[derive(
    States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default,
)]
pub enum Hearing {
    #[default]
    Off,
    On,
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

/// Writes a cue of `sound` at the listener with a number of its own,
/// so that it is not played twice alike.
pub fn write(
    cues: &mut MessageWriter<Cue>,
    turn: &mut u32,
    sound: Sound,
) {
    *turn = turn.wrapping_add(1);
    cues.write(Cue {
        sound,
        at: Vec3::ZERO,
        turn: *turn,
    });
}

/// The systems that write [`Cue`]s for what is seen. They run only
/// while the world can be heard, after the robots have taken their
/// poses.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Cues;

/// True when the world is being drawn in a window that is shown.
fn drawn(
    camera: &Query<&Camera, With<WorldCamera>>,
    window: &Query<&Window, With<PrimaryWindow>>,
) -> bool {
    camera.iter().any(|camera| camera.is_active)
        && window.iter().any(|window| window.visible)
}

/// Turns [`Hearing`] on and off with the settings and with whether
/// the world is being drawn.
fn listen(
    set: Res<SoundSettings>,
    camera: Query<&Camera, With<WorldCamera>>,
    window: Query<&Window, With<PrimaryWindow>>,
    hearing: Res<State<Hearing>>,
    mut next: ResMut<NextState<Hearing>>,
) {
    let want = if set.effects() && drawn(&camera, &window) {
        Hearing::On
    } else {
        Hearing::Off
    };
    if *hearing.get() != want {
        next.set(want);
    }
}
