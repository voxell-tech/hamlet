//! Sounds as Bevy's audio has them: a [`Sound`] made from samples and
//! kept in [`SoundAssets`], played by an [`AudioPlayer`] as its
//! [`PlaybackSettings`] say, in [`PostUpdate`]. A spatial one plays
//! at its entity, an entity of the scene, following its
//! [`Transform`], heard from the [`SpatialListener`], or without one
//! from the first [`SceneCamera`]. Changing its settings changes its
//! volume and speed; taking its player away or despawning it stops
//! it. Playing needs `audio.play`.
//!
//! ```ignore
//! fn chime(mut commands: Commands, mut sounds: ResMut<SoundAssets>) -> Result<(), String> {
//!     let samples = (0..16_000)
//!         .map(|i| (i as f32 * 0.07).sin() * (1.0 - i as f32 / 16_000.0))
//!         .collect::<Vec<_>>();
//!     let chime = sounds.add(Sound::new(&samples, 1, 32_000)?);
//!     commands.spawn((AudioPlayer(chime), PlaybackSettings::DESPAWN));
//!     Ok(())
//! }
//! ```

use std::time::Instant;

use bevy_app::{App, Last, PostUpdate};
use bevy_ecs::prelude::*;
use bevy_transform::components::Transform;
pub use hamlet_engine_api::audio::Category;

use crate::bindings::hamlet::engine::audio::{
    self as wit, Category as Kind, Clip, Playback, Sound as HostSound,
};
use crate::scene::{
    Handle, HostEntity, SceneCamera, SceneSync, Store, animate,
};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<SoundAssets>()
        .init_resource::<SoundsEnding>()
        .add_observer(stop)
        .add_systems(
            PostUpdate,
            (listen, play, set, finish, animate)
                .chain()
                .after(SceneSync),
        )
        .add_systems(Last, free_unused);
}

/// Samples made into a clip in the engine: floats from -1 to 1,
/// interleaved, `channels` (1, or 2 with the left first) a frame and
/// `rate` frames a second.
pub struct Sound {
    clip: Clip,
    seconds: f32,
}

impl Sound {
    /// The clip of `samples`, or why the engine refused it.
    pub fn new(
        samples: &[f32],
        channels: u8,
        rate: u32,
    ) -> Result<Self, String> {
        let clip = Clip::make(samples, channels, rate)?;
        let frames = samples.len() / usize::from(channels.max(1));
        Ok(Self {
            clip,
            seconds: frames as f32 / rate.max(1) as f32,
        })
    }

    /// Its length in seconds, played as made.
    pub fn seconds(&self) -> f32 {
        self.seconds
    }
}

/// The sounds the plugin made, each kept while a [`Handle`] of it is
/// held, and let go in [`Last`] after the last is dropped.
#[derive(Resource, Default)]
pub struct SoundAssets(Store<Sound>);

impl SoundAssets {
    pub fn add(&mut self, sound: Sound) -> Handle<Sound> {
        self.0.add(sound)
    }

    pub fn get(&self, handle: &Handle<Sound>) -> Option<&Sound> {
        self.0.get(handle)
    }
}

/// Plays a [`Sound`] as the entity's [`PlaybackSettings`] say.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[require(PlaybackSettings)]
pub struct AudioPlayer(pub Handle<Sound>);

/// What becomes of a sound and its entity at its end.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PlaybackMode {
    /// Plays once, and the entity stays.
    #[default]
    Once,
    /// Plays again from the start, until it is stopped.
    Loop,
    /// Plays once, and the entity is despawned.
    Despawn,
    /// Plays once, and the player and its settings are taken away.
    Remove,
}

/// How an [`AudioPlayer`] plays.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct PlaybackSettings {
    pub mode: PlaybackMode,
    /// 0 to 1, of the sound as made.
    pub volume: f32,
    /// How fast it plays, from 1/4 to 4, 1 as made; its pitch moves
    /// with it.
    pub speed: f32,
    /// At the entity, an entity of the scene, rather than the same
    /// wherever the listener is. Read as it starts.
    pub spatial: bool,
    /// Read as it starts.
    pub category: Category,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self::ONCE
    }
}

impl PlaybackSettings {
    pub const ONCE: Self = Self {
        mode: PlaybackMode::Once,
        volume: 1.0,
        speed: 1.0,
        spatial: false,
        category: Category::World,
    };
    pub const LOOP: Self = Self {
        mode: PlaybackMode::Loop,
        ..Self::ONCE
    };
    pub const DESPAWN: Self = Self {
        mode: PlaybackMode::Despawn,
        ..Self::ONCE
    };
    pub const REMOVE: Self = Self {
        mode: PlaybackMode::Remove,
        ..Self::ONCE
    };

    pub fn with_volume(self, volume: f32) -> Self {
        Self { volume, ..self }
    }

    pub fn with_speed(self, speed: f32) -> Self {
        Self { speed, ..self }
    }

    pub fn with_spatial(self, spatial: bool) -> Self {
        Self { spatial, ..self }
    }

    pub fn with_category(self, category: Category) -> Self {
        Self { category, ..self }
    }

    fn playback(&self) -> Playback {
        Playback {
            category: match self.category {
                Category::Interface => Kind::Interface,
                Category::World => Kind::World,
                Category::Music => Kind::Music,
                Category::Voices => Kind::Voices,
            },
            volume: self.volume,
            speed: self.speed,
            looped: self.mode == PlaybackMode::Loop,
        }
    }
}

/// The entity spatial sounds are heard from, at its [`Transform`].
#[derive(Component, Clone, Copy, Default, Debug)]
#[require(Transform)]
pub struct SpatialListener;

/// The engine's sound of an [`AudioPlayer`], and how much of it
/// played.
#[derive(Component)]
struct Playing {
    sound: HostSound,
    /// Seconds of the sound played, and its length.
    played: f32,
    length: f32,
    last: Instant,
    ended: bool,
}

/// Whether a sound that does not loop still plays, so the app is
/// updated each frame to end it.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct SoundsEnding(pub(crate) bool);

/// An [`AudioPlayer`] the engine would not play, not asked again.
#[derive(Component)]
struct Unplayed;

/// Hears spatial sounds from the [`SpatialListener`], else the first
/// [`SceneCamera`], as it changes.
fn listen(
    listeners: Query<(Entity, &HostEntity), With<SpatialListener>>,
    cameras: Query<(Entity, &HostEntity), With<SceneCamera>>,
    mut heard: Local<Option<Entity>>,
) {
    let at =
        listeners.iter().next().or_else(|| cameras.iter().next());
    if at.map(|(entity, _)| entity) == *heard {
        return;
    }
    *heard = at.map(|(entity, _)| entity);
    wit::listen(at.map(|(_, host)| &host.entity));
}

fn play(
    mut commands: Commands,
    players: Query<
        (
            Entity,
            &AudioPlayer,
            &PlaybackSettings,
            Has<Transform>,
            Option<&HostEntity>,
        ),
        (Without<Playing>, Without<Unplayed>),
    >,
    sounds: Res<SoundAssets>,
) {
    for (entity, player, settings, placed, host) in &players {
        let Some(sound) = sounds.get(&player.0) else {
            continue;
        };
        let playback = settings.playback();
        let played = match (settings.spatial && placed, host) {
            (false, _) => HostSound::play(&sound.clip, playback),
            (true, Some(host)) => HostSound::play_on(
                &sound.clip,
                playback,
                &host.entity,
            ),
            // Its entity is not made in the scene yet.
            (true, None) => continue,
        };
        match played {
            Ok(played) => {
                commands.entity(entity).insert(Playing {
                    sound: played,
                    played: 0.0,
                    length: sound.seconds,
                    last: Instant::now(),
                    ended: false,
                });
            }
            Err(why) => {
                crate::error(&format!(
                    "a sound is not played: {why}"
                ));
                commands.entity(entity).insert(Unplayed);
            }
        }
    }
}

fn set(
    changed: Query<
        (&Playing, Ref<PlaybackSettings>),
        Changed<PlaybackSettings>,
    >,
) {
    for (playing, settings) in &changed {
        if !settings.is_added() {
            playing.sound.set(settings.volume, settings.speed);
        }
    }
}

/// Ends each sound that is not looped at its end, as its mode says.
fn finish(
    mut commands: Commands,
    mut playing: Query<(Entity, &mut Playing, &PlaybackSettings)>,
    mut ending: ResMut<SoundsEnding>,
) {
    let now = Instant::now();
    let mut still = false;
    for (entity, mut playing, settings) in &mut playing {
        let since = now.duration_since(playing.last).as_secs_f32();
        playing.last = now;
        if playing.ended || settings.mode == PlaybackMode::Loop {
            continue;
        }
        playing.played += since * settings.speed;
        if playing.played < playing.length {
            still = true;
            continue;
        }
        playing.ended = true;
        match settings.mode {
            PlaybackMode::Despawn => {
                commands.entity(entity).despawn();
            }
            PlaybackMode::Remove => {
                commands.entity(entity).remove::<(
                    AudioPlayer,
                    PlaybackSettings,
                    Playing,
                )>();
            }
            PlaybackMode::Once | PlaybackMode::Loop => {}
        }
    }
    ending.set_if_neq(SoundsEnding(still));
}

/// Stops the sound of a player taken away.
fn stop(removed: On<Remove, AudioPlayer>, mut commands: Commands) {
    if let Ok(mut entity) = commands.get_entity(removed.entity) {
        entity.try_remove::<(Playing, Unplayed)>();
    }
}

fn free_unused(mut sounds: ResMut<SoundAssets>) {
    sounds.bypass_change_detection().0.free_unused();
}
