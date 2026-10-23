//! One plugin's clips and sounds as its host holds them while it is
//! called: each make and play held to `audio.play` and to the audio
//! memory the person switched on, and kept as a change for the world
//! once the call ends.

use std::collections::HashMap;
use std::time::Instant;

use bevy::math::Vec3;
use hamlet_engine_api::PluginLimits;
use hamlet_engine_api::audio::Category;

use crate::clip::Clip;

/// The speeds a sound may play at, as made at 1.
pub const SPEEDS: (f32, f32) = (0.25, 4.0);

/// What a plugin may hold, when the person switched it on.
#[derive(Clone, Copy, Debug)]
pub struct AudioCaps {
    /// Bytes of the samples of its clips, four a sample.
    pub clip_bytes: Option<u64>,
}

impl AudioCaps {
    pub fn of(limits: &PluginLimits) -> Self {
        Self {
            clip_bytes: limits.audio_memory,
        }
    }
}

/// How a sound plays.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Playback {
    pub category: Category,
    /// 0 to 1, of the sound as made.
    pub volume: f32,
    /// How fast it plays, 1 as made; its pitch moves with it.
    pub speed: f32,
    pub looped: bool,
}

impl Playback {
    /// Held to its bounds, or `None` for a number that is not
    /// finite.
    pub fn bounded(self) -> Option<Self> {
        (self.volume.is_finite() && self.speed.is_finite()).then(
            || Self {
                volume: self.volume.clamp(0.0, 1.0),
                speed: self.speed.clamp(SPEEDS.0, SPEEDS.1),
                ..self
            },
        )
    }
}

/// Where a sound is heard from, by its owner's numbers of its scene.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Place {
    /// The same wherever the listener is.
    Unplaced,
    /// At a point of the owner's scene.
    Point(Vec3),
    /// At an entity of the owner's scene, following it.
    Entity(u32),
}

/// A change to an owner's sounds, made once its call ends.
#[derive(Clone, Debug)]
pub enum AudioChange {
    Clip {
        id: u32,
        clip: Clip,
    },
    DropClip(u32),
    Play {
        id: u32,
        clip: u32,
        playback: Playback,
        place: Place,
    },
    Set {
        id: u32,
        volume: f32,
        speed: f32,
    },
    Stop(u32),
    /// The entity of the owner's scene its placed sounds are heard
    /// from, or none for its scene's origin.
    Listen(Option<u32>),
}

/// One plugin's clips and sounds, counted against its [`AudioCaps`].
#[derive(Debug)]
pub struct Ledger {
    /// When the numbering began; sounds of another numbering are
    /// stopped first.
    pub since: Instant,
    next: u32,
    /// The bytes of each clip held.
    clips: HashMap<u32, u64>,
    /// Clips let go of while a sound still plays them, counted until
    /// none does.
    gone: HashMap<u32, u64>,
    /// The clip each sound playing plays.
    sounds: HashMap<u32, u32>,
    bytes: u64,
    pub caps: AudioCaps,
    /// Whether it may play, and whether the person is to be asked.
    pub allowed: bool,
    pub asking: bool,
    /// It tried to play while the person had not answered.
    pub wanted: bool,
    pub changes: Vec<AudioChange>,
}

impl Ledger {
    pub fn new(caps: AudioCaps) -> Self {
        Self {
            since: Instant::now(),
            next: 0,
            clips: HashMap::new(),
            gone: HashMap::new(),
            sounds: HashMap::new(),
            bytes: 0,
            caps,
            allowed: false,
            asking: false,
            wanted: false,
            changes: Vec::new(),
        }
    }

    /// Whether it may use `audio.play`; why not otherwise.
    fn may(&mut self) -> Result<(), String> {
        if self.allowed {
            return Ok(());
        }
        if self.asking {
            self.wanted = true;
            return Err("audio.play waits for the person to allow \
                        it"
            .into());
        }
        Err("audio.play is not allowed".into())
    }

    fn number(&mut self) -> u32 {
        self.next += 1;
        self.next
    }

    /// Holds `clip`, numbered, if it may and it fits; why not
    /// otherwise.
    pub fn make_clip(&mut self, clip: Clip) -> Result<u32, String> {
        self.may()?;
        let bytes = clip.bytes();
        if let Some(most) = self.caps.clip_bytes
            && self.bytes.saturating_add(bytes) > most
        {
            return Err(format!(
                "a plugin's clips hold {most} bytes at most"
            ));
        }
        self.bytes = self.bytes.saturating_add(bytes);
        let id = self.number();
        self.clips.insert(id, bytes);
        self.changes.push(AudioChange::Clip { id, clip });
        Ok(id)
    }

    /// Lets go of the clip `id`. Its bytes stay counted while a sound
    /// plays it.
    pub fn drop_clip(&mut self, id: u32) {
        let Some(bytes) = self.clips.remove(&id) else {
            return;
        };
        if self.sounds.values().any(|clip| *clip == id) {
            self.gone.insert(id, bytes);
        } else {
            self.bytes -= bytes;
        }
        self.changes.push(AudioChange::DropClip(id));
    }

    /// Whether `id` is a clip it holds.
    pub fn holds(&self, id: u32) -> bool {
        self.clips.contains_key(&id)
    }

    /// Plays the clip `clip`, numbered, if it may and it fits; why
    /// not otherwise.
    pub fn play(
        &mut self,
        clip: u32,
        playback: Playback,
        place: Place,
    ) -> Result<u32, String> {
        self.may()?;
        let id = self.number();
        self.sounds.insert(id, clip);
        self.changes.push(AudioChange::Play {
            id,
            clip,
            playback,
            place,
        });
        Ok(id)
    }

    /// Sets how loud and how fast the sound `id` plays, while it
    /// plays.
    pub fn set(&mut self, id: u32, volume: f32, speed: f32) {
        if self.sounds.contains_key(&id) {
            self.changes.push(AudioChange::Set { id, volume, speed });
        }
    }

    /// Stops the sound `id`.
    pub fn stop(&mut self, id: u32) {
        if self.end(id) {
            self.changes.push(AudioChange::Stop(id));
        }
    }

    pub fn listen(&mut self, at: Option<u32>) {
        self.changes.push(AudioChange::Listen(at));
    }

    /// Forgets the sounds of `ids`, which came to their end.
    pub fn ended(&mut self, ids: impl IntoIterator<Item = u32>) {
        for id in ids {
            self.end(id);
        }
    }

    /// Forgets the sound `id`, and frees its clip if it was let go
    /// of and no other sound plays it: whether it was playing.
    fn end(&mut self, id: u32) -> bool {
        let Some(clip) = self.sounds.remove(&id) else {
            return false;
        };
        if !self.sounds.values().any(|other| *other == clip)
            && let Some(bytes) = self.gone.remove(&clip)
        {
            self.bytes -= bytes;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(frames: usize) -> Clip {
        Clip::new(vec![0.0; frames], 1, 32_000).unwrap()
    }

    fn ledger() -> Ledger {
        let mut ledger = Ledger::new(AudioCaps {
            clip_bytes: Some(4 * 1_000),
        });
        ledger.allowed = true;
        ledger
    }

    const ONCE: Playback = Playback {
        category: Category::World,
        volume: 1.0,
        speed: 1.0,
        looped: false,
    };

    #[test]
    fn play_is_refused_without_the_grant() {
        let mut ledger = ledger();
        let made = ledger.make_clip(clip(10)).unwrap();
        ledger.allowed = false;
        assert!(ledger.make_clip(clip(10)).is_err());
        assert!(ledger.play(made, ONCE, Place::Unplaced).is_err());
        assert!(!ledger.wanted);
        ledger.asking = true;
        assert!(ledger.play(made, ONCE, Place::Unplaced).is_err());
        assert!(ledger.wanted);
        assert_eq!(ledger.changes.len(), 1);
    }

    #[test]
    fn the_audio_memory_switched_on_refuses_past_it() {
        let mut ledger = ledger();
        let made = ledger.make_clip(clip(600)).unwrap();
        assert!(ledger.make_clip(clip(500)).is_err());

        // A clip let go of while it plays stays counted until it
        // ends.
        let first = ledger.play(made, ONCE, Place::Unplaced).unwrap();
        ledger.drop_clip(made);
        assert!(ledger.make_clip(clip(500)).is_err());
        ledger.ended([first]);
        assert!(ledger.make_clip(clip(500)).is_ok());
    }
}
