//! The realm's sound: clips made in code, played where a [`Cue`]
//! says and heard from near the ground the camera looks at, with
//! birds, crickets and wind under them. The engine scales them by
//! its own volumes and the person's.

mod bank;
mod heard;
pub mod kinds;
mod mix;
mod synth;

use glam::Vec3;
use hamlet_engine_bevy::prelude::*;

pub use crate::sound::kinds::Sound;

const WORLD: &str = include_str!("../../../assets/world.toml");
const EFFECTS: &str = include_str!("../../../assets/vfx.toml");

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<Cue>()
        .init_resource::<bank::Bank>()
        .init_resource::<heard::DayPart>()
        .init_resource::<heard::Lives>()
        .init_resource::<heard::Due>()
        .init_resource::<mix::Hearing>()
        .init_resource::<mix::Heard>()
        .add_systems(Startup, (read, mix::spawn_ears).chain())
        .add_systems(
            Update,
            (
                heard::turn_of_day,
                (heard::state_cues, heard::life_cues).chain().run_if(
                    resource_changed::<crate::society::Society>,
                ),
            )
                .chain()
                .in_set(Cues)
                .run_if(resource_exists::<SoundSettings>),
        )
        .add_systems(
            Update,
            (
                mix::listen,
                bank::make_clips,
                mix::place_ears,
                heard::follow,
                heard::hear_folk,
                heard::hear_agents,
                mix::start_ambience,
                mix::mix_ambience,
                mix::play,
                mix::end_voices,
            )
                .chain()
                .in_set(Mix)
                .run_if(resource_exists::<SoundSettings>),
        );
}

/// The systems that write [`Cue`]s of what the realm itself hears
/// happen: the turn of the day, the calls for the chief and the
/// lives of agents and hamlets. They run after the realm's frame.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Cues;

/// The systems that play the frame's cues, after every [`Cue`] of
/// it is written.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mix;

/// A sound that has just been made somewhere in the realm, written
/// by any system of the realm's frame.
///
/// A house that begins to rise is cued as [`Sound::HouseRise`], one
/// that begins to fall as [`Sound::HouseShudder`], and one merged
/// away as [`Sound::MergeAway`], each with the agent's id as `turn`:
/// what follows each is cued here.
#[derive(Message, Clone, Copy, Debug)]
pub struct Cue {
    pub sound: Sound,
    pub at: Vec3,
    /// A number that differs from one cue to the next, to vary the
    /// pitch.
    pub turn: u32,
}

/// The share of one family of sound heard at a time.
#[derive(Clone, Copy, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limit {
    /// The most of the family playing at once.
    pub most: usize,
    /// Seconds before the same sound may play again.
    pub gap: f32,
}

/// The [`Limit`] of each family of sound.
#[derive(Clone, Copy, PartialEq, Debug, serde::Deserialize)]
pub struct Limits {
    pub steps: Limit,
    pub work: Limit,
    pub doors: Limit,
    pub alerts: Limit,
    pub hand: Limit,
    pub magic: Limit,
    pub talk: Limit,
    pub lives: Limit,
    pub answers: Limit,
}

/// The realm's sound, `[sound]` of `assets/world.toml`. Its reach and
/// the distance between the ears are the engine's.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
pub struct SoundSettings {
    /// The volume of each family of sound, before the engine's:
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
    pub limits: Limits,
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
    /// Seconds a merged house's motes fly to the house they join,
    /// `[magic] flight_time` of `assets/vfx.toml`.
    #[serde(skip)]
    pub flight_time: f32,
}

/// The section of `assets/world.toml` the sound reads.
#[derive(serde::Deserialize)]
struct World {
    sound: SoundSettings,
}

/// The section of `assets/vfx.toml` the sound reads.
#[derive(serde::Deserialize)]
struct Effects {
    magic: Magic,
}

#[derive(serde::Deserialize)]
struct Magic {
    flight_time: f32,
}

impl SoundSettings {
    /// The sound's tuning as the files built in have it, or why it
    /// cannot be read.
    fn read() -> Result<Self, String> {
        let mut set = toml::from_str::<World>(WORLD)
            .map_err(|why| format!("assets/world.toml: {why}"))?
            .sound;
        set.flight_time = toml::from_str::<Effects>(EFFECTS)
            .map_err(|why| format!("assets/vfx.toml: {why}"))?
            .magic
            .flight_time;
        Ok(set)
    }
}

/// Reads the sound's tuning. Without it the realm is silent.
fn read(mut commands: Commands) {
    match SoundSettings::read() {
        Ok(set) => commands.insert_resource(set),
        Err(why) => hamlet_engine_bevy::error(&format!(
            "the realm is silent: {why}"
        )),
    }
}
