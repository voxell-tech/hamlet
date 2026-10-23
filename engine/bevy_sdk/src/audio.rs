//! Sounds as Bevy's audio has them: a [`Sound`] made from samples and
//! kept in [`SoundAssets`], played by an [`AudioPlayer`] as its
//! [`PlaybackSettings`] say, in [`PostUpdate`]. A spatial one plays
//! at its entity, an entity of the scene, following its
//! [`Transform`], heard from the [`SpatialListener`], or without one
//! from the first [`SceneCamera`]. Changing its settings changes its
//! volume and speed, a [`FadeTo`] moves its volume evenly, and taking
//! its player away or despawning it stops it. Playing needs
//! `audio.play`.
//!
//! The samples of a sound are handed to the engine in parts, as many
//! each update as the engine takes, so a sound of any length is made
//! at once and may play while it is still handed over. A
//! [`Sound::stream`] is filled with [`SoundAssets::append`] as it
//! plays, and let go of in the engine as it is heard.
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

use bevy_app::{App, Last, PostUpdate};
use bevy_ecs::prelude::*;
use bevy_transform::components::Transform;
pub use hamlet_engine_api::audio::Category;

use crate::bindings::hamlet::engine::audio::{
    self as wit, AppendError, Category as Kind, Clip, Playback,
    Sound as HostSound,
};
use crate::scene::{
    Handle, HostEntity, SceneCamera, SceneSync, Store, animate,
};

/// Samples handed to the engine in one call: a quarter of a MiB.
const PART: usize = 65_536;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<SoundAssets>()
        .init_resource::<SoundsEnding>()
        .add_observer(stop)
        .add_systems(
            PostUpdate,
            (feed, listen, play, set, fade, finish, animate)
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
    channels: u8,
    /// Its length as made, 0 for one opened to be filled.
    seconds: f32,
    /// Samples to hand over once it is added, and whether it is
    /// closed after them.
    samples: Vec<f32>,
    closed: bool,
}

impl Sound {
    /// The sound of `samples`, or why the engine refused it.
    pub fn new(
        samples: &[f32],
        channels: u8,
        rate: u32,
    ) -> Result<Self, String> {
        if samples.is_empty() {
            return Err("a sound has samples".into());
        }
        let mut sound = Self::opened(channels, rate, false)?;
        check(samples, channels)?;
        let frames = samples.len() / usize::from(channels.max(1));
        sound.seconds = frames as f32 / rate.max(1) as f32;
        sound.samples = samples.to_vec();
        sound.closed = true;
        Ok(sound)
    }

    /// Its length in seconds, played as made; 0 for one opened to be
    /// filled.
    pub fn seconds(&self) -> f32 {
        self.seconds
    }

    /// An empty sound, filled with [`SoundAssets::append`] until
    /// [`SoundAssets::close`]. It may play, looped or not, while it
    /// is filled: past the samples handed over it waits in
    /// silence, and it loops once closed.
    pub fn open(channels: u8, rate: u32) -> Result<Self, String> {
        Self::opened(channels, rate, false)
    }

    /// An empty sound filled as [`Sound::open`]'s, played once by one
    /// [`AudioPlayer`], not looped. The engine lets go of its samples
    /// as they are heard, so a long one holds little.
    pub fn stream(channels: u8, rate: u32) -> Result<Self, String> {
        Self::opened(channels, rate, true)
    }

    fn opened(
        channels: u8,
        rate: u32,
        once: bool,
    ) -> Result<Self, String> {
        Ok(Self {
            clip: Clip::open(channels, rate, once)?,
            channels,
            seconds: 0.0,
            samples: Vec::new(),
            closed: false,
        })
    }
}

/// Why `samples` are not whole finite frames of `channels`.
fn check(samples: &[f32], channels: u8) -> Result<(), String> {
    if !samples.len().is_multiple_of(usize::from(channels.max(1))) {
        return Err(format!(
            "{} samples are not whole frames of {channels} channels",
            samples.len()
        ));
    }
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err("a sample is not a finite number".into());
    }
    Ok(())
}

/// A sound's samples not yet handed over.
struct Filling {
    sound: Handle<Sound>,
    channels: u8,
    samples: Vec<f32>,
    /// Handed over from here on.
    at: usize,
    closed: bool,
}

/// The sounds the plugin made, each kept while a [`Handle`] of it is
/// held, and let go in [`Last`] after the last is dropped. One still
/// open is held until it is closed and handed over.
#[derive(Resource, Default)]
pub struct SoundAssets {
    store: Store<Sound>,
    /// In the order they were added.
    filling: Vec<Filling>,
}

impl SoundAssets {
    pub fn add(&mut self, mut sound: Sound) -> Handle<Sound> {
        let samples = core::mem::take(&mut sound.samples);
        let (channels, closed) = (sound.channels, sound.closed);
        let handle = self.store.add(sound);
        self.filling.push(Filling {
            sound: handle.clone(),
            channels,
            samples,
            at: 0,
            closed,
        });
        handle
    }

    pub fn get(&self, handle: &Handle<Sound>) -> Option<&Sound> {
        self.store.get(handle)
    }

    /// Adds `samples` at the end of `sound`, opened and not closed,
    /// to be handed over as the engine takes them.
    pub fn append(
        &mut self,
        sound: &Handle<Sound>,
        samples: &[f32],
    ) -> Result<(), String> {
        let Some(filling) = self.filling_mut(sound) else {
            return Err("the sound is not open".into());
        };
        check(samples, filling.channels)?;
        filling.samples.extend_from_slice(samples);
        Ok(())
    }

    /// Ends `sound` after the samples appended: it is heard to its
    /// end, and a looped one starts again.
    pub fn close(&mut self, sound: &Handle<Sound>) {
        if let Some(filling) = self.filling_mut(sound) {
            filling.closed = true;
        }
    }

    fn filling_mut(
        &mut self,
        sound: &Handle<Sound>,
    ) -> Option<&mut Filling> {
        self.filling.iter_mut().find(|filling| {
            filling.sound == *sound && !filling.closed
        })
    }

    /// Hands over what the engine takes of the samples of each sound
    /// filled, and closes those done.
    fn feed(&mut self) {
        let mut full = false;
        self.filling.retain_mut(|filling| {
            let Some(sound) = self.store.get(&filling.sound) else {
                return false;
            };
            let channels = usize::from(filling.channels.max(1));
            let part = PART - PART % channels;
            while !full && filling.at < filling.samples.len() {
                let end =
                    (filling.at + part).min(filling.samples.len());
                match sound
                    .clip
                    .append(&filling.samples[filling.at..end])
                {
                    Ok(()) => filling.at = end,
                    // Handed over again in a later update.
                    Err(AppendError::Full) => full = true,
                    Err(AppendError::Refused(why)) => {
                        crate::error(&format!(
                            "a sound's samples are refused: {why}"
                        ));
                        return false;
                    }
                }
            }
            if filling.at < filling.samples.len() {
                return true;
            }
            filling.samples.clear();
            filling.at = 0;
            if filling.closed {
                sound.clip.close();
            }
            !filling.closed
        });
    }

    /// Whether samples wait to be handed over.
    fn waiting(&self) -> bool {
        self.filling
            .iter()
            .any(|filling| filling.at < filling.samples.len())
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

/// Moves the volume of its entity's [`AudioPlayer`] evenly to
/// `volume` over `seconds`, in the engine. It is taken away once the
/// sound plays, and the [`PlaybackSettings`] hold `volume`.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct FadeTo {
    pub volume: f32,
    pub seconds: f32,
}

/// The entity spatial sounds are heard from, at its [`Transform`].
#[derive(Component, Clone, Copy, Default, Debug)]
#[require(Transform)]
pub struct SpatialListener;

/// The engine's sound of an [`AudioPlayer`].
#[derive(Component)]
struct Playing {
    sound: HostSound,
    ended: bool,
}

/// Whether a sound that does not loop still plays, or samples wait
/// to be handed over, so the app is updated each frame.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct SoundsEnding(pub(crate) bool);

/// An [`AudioPlayer`] the engine would not play, not asked again.
#[derive(Component)]
struct Unplayed;

fn feed(mut sounds: ResMut<SoundAssets>) {
    if !sounds.filling.is_empty() {
        sounds.feed();
    }
}

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

fn fade(
    mut commands: Commands,
    mut fading: Query<(
        Entity,
        &Playing,
        &FadeTo,
        &mut PlaybackSettings,
    )>,
) {
    for (entity, playing, to, mut settings) in &mut fading {
        playing.sound.fade(to.volume, to.seconds);
        settings.bypass_change_detection().volume = to.volume;
        commands.entity(entity).remove::<FadeTo>();
    }
}

/// Ends each sound that is not looped once the engine says it ended,
/// as its mode says.
fn finish(
    mut commands: Commands,
    mut playing: Query<(Entity, &mut Playing, &PlaybackSettings)>,
    sounds: Res<SoundAssets>,
    mut ending: ResMut<SoundsEnding>,
) {
    let mut still = sounds.waiting();
    for (entity, mut playing, settings) in &mut playing {
        if playing.ended || settings.mode == PlaybackMode::Loop {
            continue;
        }
        if playing.sound.playing() {
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
        entity.try_remove::<(Playing, Unplayed, FadeTo)>();
    }
}

fn free_unused(mut sounds: ResMut<SoundAssets>) {
    sounds.bypass_change_detection().store.free_unused();
}
