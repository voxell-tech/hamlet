//! A plugin's sounds: its clips and sounds held to `audio.play` and
//! the budgets by its [`Ledger`](hamlet_engine_audio::Ledger) while
//! it is called, kept in step
//! with the grants, and made in the world once its call ends.

use core::fmt;
use std::collections::HashSet;

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::query::{With, Without};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::ecs::world::World;
use bevy::math::Vec3;
use hamlet_engine_api::Capability;
use hamlet_engine_api::audio::{Category, OUTPUT};
use hamlet_engine_audio::{
    Audio, AudioWorldExt, Clip, Place, Playback,
};
use hamlet_engine_grants::{Decided, FirstUse, Grants, Verdict};
use hamlet_engine_plugins::PluginInfo;
use hamlet_engine_render::SceneWorldExt;
use wasmtime::component::Resource;

use crate::audio::audio_bindings::hamlet::engine::audio::{
    Category as Kind, Host, HostClip, HostSound, Playback as Asked,
};
use crate::instance::{Instance, State};
use crate::panel::serve;
use crate::scene::{EntityHandle, SceneError, apply_scenes};

pub(crate) mod audio_bindings {
    wasmtime::component::bindgen!({
        path: "../wit/hamlet_engine.wit",
        world: "sounds",
        imports: { default: trappable },
        with: {
            "hamlet:engine/ui": crate::instance::ui_bindings::hamlet::engine::ui,
            "hamlet:engine/scene": crate::scene::scene_bindings::hamlet::engine::scene,
            "hamlet:engine/audio.clip": super::ClipHandle,
            "hamlet:engine/audio.sound": super::SoundHandle,
        },
    });
}

/// The interface of sounds a plugin may import.
pub(crate) const AUDIO: &str = "hamlet:engine/audio@0.1.0";

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, grant.before(serve)).add_systems(
        Update,
        (apply, forget).chain().after(serve).after(apply_scenes),
    );
}

pub struct ClipHandle;
pub struct SoundHandle;

/// A plugin gave a sound a number that is not finite.
#[derive(Clone, Copy, PartialEq, Debug)]
struct NotFinite;

impl fmt::Display for NotFinite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "it gave a sound a number that is not finite")
    }
}

impl core::error::Error for NotFinite {}

fn playback(asked: Asked) -> wasmtime::Result<Playback> {
    let category = match asked.category {
        Kind::Interface => Category::Interface,
        Kind::World => Category::World,
        Kind::Music => Category::Music,
        Kind::Voices => Category::Voices,
    };
    Playback {
        category,
        volume: asked.volume,
        speed: asked.speed,
        looped: asked.looped,
    }
    .bounded()
    .ok_or_else(|| wasmtime::Error::new(NotFinite))
}

impl State {
    /// Plays the clip `clip` from `place`, within the budgets.
    fn play(
        &mut self,
        clip: &Resource<ClipHandle>,
        asked: Asked,
        place: Place,
    ) -> wasmtime::Result<Result<Resource<SoundHandle>, String>> {
        let id = clip.rep();
        if !self.audio.holds(id) {
            return Err(wasmtime::Error::new(SceneError::Unknown));
        }
        let playback = playback(asked)?;
        Ok(self
            .audio
            .play(id, playback, place)
            .map(Resource::new_own))
    }

    /// The scene's number of `at`, an entity of the plugin's.
    fn entity_of(
        &self,
        at: &Resource<EntityHandle>,
    ) -> wasmtime::Result<u32> {
        let id = at.rep();
        if self.scene.holds_entity(id) {
            Ok(id)
        } else {
            Err(wasmtime::Error::new(SceneError::Unknown))
        }
    }
}

impl Host for State {
    fn listen(
        &mut self,
        at: Option<Resource<EntityHandle>>,
    ) -> wasmtime::Result<()> {
        let at = at.map(|at| self.entity_of(&at)).transpose()?;
        self.audio.listen(at);
        Ok(())
    }
}

impl HostClip for State {
    fn make(
        &mut self,
        samples: Vec<f32>,
        channels: u8,
        rate: u32,
    ) -> wasmtime::Result<Result<Resource<ClipHandle>, String>> {
        self.keep_bytes(size_of_val(samples.as_slice()))?;
        let clip = match Clip::new(samples, channels.into(), rate) {
            Ok(clip) => clip,
            Err(why) => return Ok(Err(why)),
        };
        Ok(self.audio.make_clip(clip).map(Resource::new_own))
    }

    fn drop(
        &mut self,
        clip: Resource<ClipHandle>,
    ) -> wasmtime::Result<()> {
        self.audio.drop_clip(clip.rep());
        Ok(())
    }
}

impl HostSound for State {
    fn play(
        &mut self,
        clip: Resource<ClipHandle>,
        playback: Asked,
    ) -> wasmtime::Result<Result<Resource<SoundHandle>, String>> {
        State::play(self, &clip, playback, Place::Unplaced)
    }

    fn play_at(
        &mut self,
        clip: Resource<ClipHandle>,
        playback: Asked,
        point: (f32, f32, f32),
    ) -> wasmtime::Result<Result<Resource<SoundHandle>, String>> {
        let point = Vec3::from(point);
        if !point.is_finite() {
            return Err(wasmtime::Error::new(NotFinite));
        }
        State::play(self, &clip, playback, Place::Point(point))
    }

    fn play_on(
        &mut self,
        clip: Resource<ClipHandle>,
        playback: Asked,
        at: Resource<EntityHandle>,
    ) -> wasmtime::Result<Result<Resource<SoundHandle>, String>> {
        let at = self.entity_of(&at)?;
        State::play(self, &clip, playback, Place::Entity(at))
    }

    fn set(
        &mut self,
        sound: Resource<SoundHandle>,
        volume: f32,
        speed: f32,
    ) -> wasmtime::Result<()> {
        let asked = Asked {
            category: Kind::World,
            volume,
            speed,
            looped: false,
        };
        let bounded = playback(asked)?;
        self.audio.set(sound.rep(), bounded.volume, bounded.speed);
        Ok(())
    }

    fn stop(
        &mut self,
        sound: Resource<SoundHandle>,
    ) -> wasmtime::Result<()> {
        self.audio.stop(sound.rep());
        Ok(())
    }

    fn drop(
        &mut self,
        sound: Resource<SoundHandle>,
    ) -> wasmtime::Result<()> {
        self.audio.stop(sound.rep());
        Ok(())
    }
}

/// Lets each running plugin play as the person allows, asks the
/// person about one that tried while answered Ask next time, and
/// tells its ledger of the sounds that came to their end.
fn grant(
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &mut Instance,
        Option<&mut Audio>,
    )>,
    grants: Res<Grants>,
    mut decided: MessageReader<Decided>,
    mut first_use: MessageWriter<FirstUse>,
    mut given: Local<HashSet<Entity>>,
    mut asked: Local<HashSet<Entity>>,
) {
    for decision in decided.read() {
        if decision.capability != Capability::AudioPlay {
            continue;
        }
        asked.remove(&decision.plugin);
        if decision.allowed {
            given.insert(decision.plugin);
        }
    }
    given.retain(|plugin| plugins.contains(*plugin));
    asked.retain(|plugin| plugins.contains(*plugin));
    for (plugin, info, mut instance, audio) in &mut plugins {
        let ledger = instance.audio();
        let verdict =
            grants.verdict(info, Capability::AudioPlay, OUTPUT);
        ledger.allowed = verdict == Verdict::Allowed
            || verdict == Verdict::Asking && given.contains(&plugin);
        ledger.asking = verdict == Verdict::Asking && !ledger.allowed;
        if ledger.wanted && asked.insert(plugin) {
            first_use.write(FirstUse {
                plugin,
                capability: Capability::AudioPlay,
                target: OUTPUT.into(),
            });
        }
        ledger.wanted = false;
        if let Some(mut audio) = audio {
            let ended = audio.take_ended(ledger.since);
            ledger.ended(ended);
        }
    }
}

/// Makes what each plugin changed in its sounds, finding the
/// entities of its scene they name. A plugin numbering anew, as after
/// a reload, has its sounds stopped first.
fn apply(
    mut plugins: Query<(Entity, &mut Instance, Option<&Audio>)>,
    mut commands: Commands,
) {
    for (plugin, mut instance, audio) in &mut plugins {
        let ledger = instance.audio();
        let since = ledger.since;
        let stale = audio
            .and_then(Audio::since)
            .is_some_and(|was| was != since);
        if ledger.changes.is_empty() && !stale {
            continue;
        }
        let changes = std::mem::take(&mut ledger.changes);
        commands.queue(move |world: &mut World| {
            world.apply_audio(plugin, since, changes, |world, id| {
                world.scene_of(plugin)?.entity(id)
            });
        });
    }
}

/// Stops the sounds of plugins that no longer run.
fn forget(
    plugins: Query<Entity, (With<Audio>, Without<Instance>)>,
    mut commands: Commands,
) {
    for plugin in &plugins {
        commands.queue(move |world: &mut World| {
            world.clear_audio(plugin);
        });
    }
}
