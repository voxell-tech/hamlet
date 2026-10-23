//! Sounds playing: each an entity with Bevy's player, its volume the
//! person's settings scale, a placed one moved each frame to where
//! its owner's listener hears it, and one that is not looped taken
//! away at its end.

use std::collections::HashMap;
use std::time::Instant;

use bevy::app::{App, PostUpdate, Update};
use bevy::asset::{Assets, Handle};
use bevy::audio::{
    AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackMode,
    PlaybackSettings, SpatialAudioSink, SpatialScale, Volume,
};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::Has;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::time::Time;
use bevy::transform::TransformSystems;
use bevy::transform::components::{GlobalTransform, Transform};
use crossbeam_channel::Sender;

use crate::clip::Clip;
use crate::ledger::{AudioChange, Place, Playback};
use crate::settings::{Muted, SoundSettings, SoundTuning};

/// The gap between the ears Bevy hears from without a listener.
const BEVY_EARS: f32 = 4.0;

pub(crate) fn plugin(app: &mut App) {
    // Before Bevy starts the sounds spawned this frame, after
    // `Propagate`, so each starts as loud and where it should.
    app.add_systems(Update, finish).add_systems(
        PostUpdate,
        (mix, place).before(TransformSystems::Propagate),
    );
}

/// Where a sound is heard from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Placed {
    /// The same wherever the listener is.
    Unplaced,
    /// At a point of the world.
    Point(Vec3),
    /// At an entity, following it.
    Entity(Entity),
}

/// A sound playing, of the engine's own or of the plugin `owner`.
#[derive(Component, Clone, Debug)]
#[require(Transform)]
pub struct Sounding {
    pub owner: Option<Entity>,
    /// The owner's number of it.
    pub id: u32,
    pub playback: Playback,
    pub placed: Placed,
    /// Seconds of the clip played, and its length so far.
    played: f32,
    length: f32,
    /// Its clip is still filled: it waits at its end for more.
    filling: bool,
    /// The volume it moves to, and by how much a second.
    fade: Option<(f32, f32)>,
    /// For a clip played once: the owner's number of the clip, its
    /// bytes a second, and the bytes heard told of.
    once: Option<(u32, f32, u64)>,
}

/// On a plugin: its clips and sounds by its numbers, the entity its
/// placed sounds are heard from, and the sounds that came to their
/// end for its host to hear of.
#[derive(Component, Default, Debug)]
pub struct Audio {
    since: Option<Instant>,
    clips: HashMap<u32, Handle<Clip>>,
    /// The sounds playing each clip still filled, and where its
    /// samples are sent to each.
    feeds: HashMap<u32, Vec<(u32, Sender<Vec<f32>>)>>,
    sounds: HashMap<u32, Entity>,
    listener: Option<Entity>,
    ended: Vec<u32>,
    /// Bytes heard of each clip played once.
    heard: Vec<(u32, u64)>,
}

impl Audio {
    /// When the numbering of its sounds began.
    pub fn since(&self) -> Option<Instant> {
        self.since
    }

    /// The sounds of the numbering `since` that came to their end
    /// since this was last asked.
    pub fn take_ended(&mut self, since: Instant) -> Vec<u32> {
        let ended = std::mem::take(&mut self.ended);
        if self.since == Some(since) {
            ended
        } else {
            Vec::new()
        }
    }

    /// The bytes heard of the clips played once of the numbering
    /// `since`, since this was last asked.
    pub fn take_heard(&mut self, since: Instant) -> Vec<(u32, u64)> {
        let heard = std::mem::take(&mut self.heard);
        if self.since == Some(since) {
            heard
        } else {
            Vec::new()
        }
    }

    /// Forgets the sound `id`, which no longer plays.
    fn forget(&mut self, id: u32) -> Option<Entity> {
        for feeds in self.feeds.values_mut() {
            feeds.retain(|(sound, _)| *sound != id);
        }
        self.sounds.remove(&id)
    }
}

/// The audio service's methods on [`Commands`].
pub trait AudioCommandsExt {
    /// Plays `clip` as `playback` says, heard from `placed`.
    fn play_sound(
        &mut self,
        clip: Handle<Clip>,
        length: f32,
        playback: Playback,
        placed: Placed,
    ) -> Entity;
}

impl AudioCommandsExt for Commands<'_, '_> {
    fn play_sound(
        &mut self,
        clip: Handle<Clip>,
        length: f32,
        playback: Playback,
        placed: Placed,
    ) -> Entity {
        let sounding =
            Sounding::new(None, 0, length, playback, placed);
        self.spawn(sounding.player(clip, playback.looped)).id()
    }
}

/// The audio service's methods on [`World`].
pub trait AudioWorldExt {
    /// Makes the changes of `owner`, numbered since `since`, finding
    /// the entity of each number of its scene with `entity`. Sounds
    /// of another numbering are stopped first.
    fn apply_audio(
        &mut self,
        owner: Entity,
        since: Instant,
        changes: Vec<AudioChange>,
        entity: impl Fn(&World, u32) -> Option<Entity>,
    );

    /// Stops every sound of `owner` and lets go of its clips.
    fn clear_audio(&mut self, owner: Entity);
}

impl AudioWorldExt for World {
    fn apply_audio(
        &mut self,
        owner: Entity,
        since: Instant,
        changes: Vec<AudioChange>,
        entity: impl Fn(&World, u32) -> Option<Entity>,
    ) {
        if self
            .get::<Audio>(owner)
            .is_some_and(|audio| audio.since != Some(since))
        {
            self.clear_audio(owner);
        }
        let Ok(mut held) = self.get_entity_mut(owner) else {
            return;
        };
        let mut audio = held.take::<Audio>().unwrap_or_default();
        audio.since = Some(since);
        for change in changes {
            self.change_audio(owner, &mut audio, change, &entity);
        }
        if let Ok(mut held) = self.get_entity_mut(owner) {
            held.insert(audio);
        }
    }

    fn clear_audio(&mut self, owner: Entity) {
        let Some(audio) = self
            .get_entity_mut(owner)
            .ok()
            .and_then(|mut held| held.take::<Audio>())
        else {
            return;
        };
        for sound in audio.sounds.into_values() {
            let _ = self.try_despawn(sound);
        }
    }
}

/// The audio service's own methods on [`World`].
trait AudioChangeExt {
    fn change_audio(
        &mut self,
        owner: Entity,
        audio: &mut Audio,
        change: AudioChange,
        entity: &impl Fn(&World, u32) -> Option<Entity>,
    );

    /// Ends the filling of the clip `id` for the sounds playing it.
    fn close_feeds(&mut self, audio: &mut Audio, id: u32);
}

impl AudioChangeExt for World {
    fn change_audio(
        &mut self,
        owner: Entity,
        audio: &mut Audio,
        change: AudioChange,
        entity: &impl Fn(&World, u32) -> Option<Entity>,
    ) {
        match change {
            AudioChange::Clip { id, clip } => {
                let handle =
                    self.resource_mut::<Assets<Clip>>().add(clip);
                audio.clips.insert(id, handle);
            }
            AudioChange::Append { id, samples } => {
                let Some(handle) = audio.clips.get(&id) else {
                    return;
                };
                let Some(seconds) = self
                    .resource::<Assets<Clip>>()
                    .get(handle)
                    .map(|clip| clip.seconds_of(samples.len()))
                else {
                    return;
                };
                let feeds = audio
                    .feeds
                    .get(&id)
                    .map_or(&[][..], Vec::as_slice);
                for (sound, sender) in feeds {
                    let _ = sender.send(samples.clone());
                    if let Some(&sound) = audio.sounds.get(sound)
                        && let Some(mut sounding) =
                            self.get_mut::<Sounding>(sound)
                    {
                        sounding.length += seconds;
                    }
                }
                let mut clips = self.resource_mut::<Assets<Clip>>();
                if let Some(mut clip) = clips.get_mut(handle) {
                    clip.append(&samples);
                }
            }
            AudioChange::Close(id) => {
                if let Some(handle) = audio.clips.get(&id)
                    && let Some(mut clip) = self
                        .resource_mut::<Assets<Clip>>()
                        .get_mut(handle)
                {
                    clip.close();
                }
                self.close_feeds(audio, id);
            }
            AudioChange::DropClip(id) => {
                self.close_feeds(audio, id);
                audio.clips.remove(&id);
            }
            AudioChange::Play {
                id,
                clip,
                playback,
                place,
            } => {
                let Some(handle) = audio.clips.get(&clip).cloned()
                else {
                    return;
                };
                let Some(made) =
                    self.resource::<Assets<Clip>>().get(&handle)
                else {
                    return;
                };
                let length = made.seconds();
                let once = made.once().then(|| {
                    let rate = made.rate() as f32;
                    let bytes =
                        rate * f32::from(made.channels()) * 4.0;
                    (clip, bytes, 0)
                });
                let filling = made.filling();
                // A clip still filled is played from a copy of its
                // own, sent the rest as it comes.
                let (handle, looped) = if filling {
                    let mut clips =
                        self.resource_mut::<Assets<Clip>>();
                    let Some((fed, sender)) = clips
                        .get_mut(&handle)
                        .map(|mut made| made.fed(playback.looped))
                    else {
                        return;
                    };
                    audio
                        .feeds
                        .entry(clip)
                        .or_default()
                        .push((id, sender));
                    (clips.add(fed), false)
                } else {
                    (handle, playback.looped)
                };
                let placed = match place {
                    Place::Unplaced => Placed::Unplaced,
                    Place::Point(point) => Placed::Point(point),
                    Place::Entity(number) => entity(self, number)
                        .map_or(Placed::Unplaced, Placed::Entity),
                };
                let mut sounding = Sounding::new(
                    Some(owner),
                    id,
                    length,
                    playback,
                    placed,
                );
                sounding.filling = filling;
                sounding.once = once;
                let made =
                    self.spawn(sounding.player(handle, looped)).id();
                audio.sounds.insert(id, made);
            }
            AudioChange::Set { id, volume, speed } => {
                if let Some(&sound) = audio.sounds.get(&id)
                    && let Some(mut sounding) =
                        self.get_mut::<Sounding>(sound)
                {
                    sounding.playback.volume = volume;
                    sounding.playback.speed = speed;
                    sounding.fade = None;
                }
            }
            AudioChange::Fade {
                id,
                volume,
                seconds,
            } => {
                if let Some(&sound) = audio.sounds.get(&id)
                    && let Some(mut sounding) =
                        self.get_mut::<Sounding>(sound)
                {
                    let away =
                        (volume - sounding.playback.volume).abs();
                    if seconds > 0.0 {
                        sounding.fade =
                            Some((volume, away / seconds));
                    } else {
                        sounding.playback.volume = volume;
                        sounding.fade = None;
                    }
                }
            }
            AudioChange::Stop(id) => {
                if let Some(sound) = audio.forget(id) {
                    let _ = self.try_despawn(sound);
                }
            }
            AudioChange::Listen(at) => {
                audio.listener =
                    at.and_then(|number| entity(self, number));
            }
        }
    }

    fn close_feeds(&mut self, audio: &mut Audio, id: u32) {
        // Dropping the senders tells each playing that no more come.
        for (sound, _) in audio.feeds.remove(&id).unwrap_or_default()
        {
            if let Some(&sound) = audio.sounds.get(&sound)
                && let Some(mut sounding) =
                    self.get_mut::<Sounding>(sound)
            {
                sounding.filling = false;
            }
        }
    }
}

impl Sounding {
    fn new(
        owner: Option<Entity>,
        id: u32,
        length: f32,
        playback: Playback,
        placed: Placed,
    ) -> Self {
        Self {
            owner,
            id,
            playback,
            placed,
            played: 0.0,
            length,
            filling: false,
            fade: None,
            once: None,
        }
    }

    /// Its entity: its player, which Bevy starts in [`PostUpdate`]
    /// once [`mix`] gave it its volume, `looped` by Bevy or not.
    fn player(
        self,
        clip: Handle<Clip>,
        looped: bool,
    ) -> (Self, AudioPlayer<Clip>, PlaybackSettings) {
        let mode = if looped {
            PlaybackMode::Loop
        } else {
            PlaybackMode::Once
        };
        let settings = PlaybackSettings {
            mode,
            volume: Volume::SILENT,
            speed: self.playback.speed,
            spatial: self.placed != Placed::Unplaced,
            ..PlaybackSettings::ONCE
        };
        (self, AudioPlayer(clip), settings)
    }
}

/// Gives each sound its volume and speed: its own, moved on by its
/// fade, times the engine's and the person's volume of its category,
/// and none for a muted plugin's.
fn mix(
    time: Res<Time>,
    tuning: Res<SoundTuning>,
    settings: Res<SoundSettings>,
    owners: Query<Has<Muted>>,
    mut sounds: Query<(
        &mut Sounding,
        &mut PlaybackSettings,
        Option<&mut AudioSink>,
        Option<&mut SpatialAudioSink>,
    )>,
) {
    let dt = time.delta_secs();
    for (mut sounding, mut started, sink, spatial) in &mut sounds {
        if let Some((to, rate)) = sounding.fade {
            let from = sounding.playback.volume;
            let step = rate * dt;
            sounding.playback.volume =
                from + (to - from).clamp(-step, step);
            if sounding.playback.volume == to {
                sounding.fade = None;
            }
        }
        let playback = sounding.playback;
        let muted = sounding
            .owner
            .is_some_and(|owner| owners.get(owner).unwrap_or(true));
        let volume = if muted {
            0.0
        } else {
            playback.volume
                * tuning.volume(playback.category)
                * settings.volume(playback.category)
        };
        let volume = Volume::Linear(volume);
        if let Some(mut sink) = sink {
            set(&mut *sink, volume, playback.speed);
        } else if let Some(mut sink) = spatial {
            set(&mut *sink, volume, playback.speed);
        } else {
            // Not started yet, or with no output to start on.
            if started.volume != volume {
                started.volume = volume;
            }
            if started.speed != playback.speed {
                started.speed = playback.speed;
            }
        }
    }
}

fn set(
    sink: &mut impl AudioSinkPlayback,
    volume: Volume,
    speed: f32,
) {
    if sink.volume() != volume {
        sink.set_volume(volume);
    }
    if sink.speed() != speed {
        sink.set_speed(speed);
    }
}

/// Takes away each sound that is not looped at its end, and tells its
/// owner, with the bytes heard of each clip played once.
fn finish(
    time: Res<Time>,
    mut sounds: Query<(Entity, &mut Sounding)>,
    mut owners: Query<&mut Audio>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (entity, mut sounding) in &mut sounds {
        if sounding.playback.looped {
            continue;
        }
        let played = sounding.played + dt * sounding.playback.speed;
        // A sound of a clip still filled waits at its end for more.
        sounding.played = if sounding.filling {
            played.min(sounding.length)
        } else {
            played
        };
        let mut owner = sounding
            .owner
            .and_then(|owner| owners.get_mut(owner).ok());
        let played = sounding.played;
        if let Some((clip, rate, told)) = &mut sounding.once {
            let heard = (played * *rate) as u64;
            if heard > *told
                && let Some(audio) = owner.as_mut()
            {
                audio.heard.push((*clip, heard - *told));
                *told = heard;
            }
        }
        if sounding.filling || sounding.played < sounding.length {
            continue;
        }
        commands.entity(entity).despawn();
        if let Some(mut audio) = owner
            && audio.forget(sounding.id).is_some()
        {
            audio.ended.push(sounding.id);
        }
    }
}

/// Moves each placed sound to where its owner's listener hears it,
/// in the room about a listener Bevy hears from.
fn place(
    tuning: Res<SoundTuning>,
    owners: Query<&Audio>,
    at: Query<&GlobalTransform>,
    mut sounds: Query<(
        &Sounding,
        &mut Transform,
        &mut PlaybackSettings,
    )>,
) {
    // Bevy hears every sound from one listener with ears
    // `BEVY_EARS` apart, at the origin when there is none, as here.
    // A sound is put where it is from its owner's listener, sized so
    // that those ears are `ears` apart and `reach` away is one.
    let room = BEVY_EARS / tuning.ears;
    let scale =
        SpatialScale::new(tuning.ears / (BEVY_EARS * tuning.reach));
    for (sounding, mut transform, mut settings) in &mut sounds {
        let from = match sounding.placed {
            Placed::Unplaced => continue,
            Placed::Point(point) => Some(point),
            Placed::Entity(entity) => {
                at.get(entity).ok().map(GlobalTransform::translation)
            }
        };
        let Some(from) = from else {
            continue;
        };
        let listener = sounding
            .owner
            .and_then(|owner| owners.get(owner).ok())
            .and_then(|audio| audio.listener)
            .and_then(|listener| at.get(listener).ok())
            .map_or(GlobalTransform::IDENTITY, |listener| *listener);
        let heard =
            listener.affine().inverse().transform_point3(from);
        transform
            .set_if_neq(Transform::from_translation(heard * room));
        if settings.spatial_scale.map(|was| was.0) != Some(scale.0) {
            settings.spatial_scale = Some(scale);
        }
    }
}
