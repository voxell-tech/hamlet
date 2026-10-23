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
    /// Samples, checked, added at the end of a clip being filled.
    Append {
        id: u32,
        samples: Vec<f32>,
    },
    Close(u32),
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
    Fade {
        id: u32,
        volume: f32,
        seconds: f32,
    },
    Stop(u32),
    /// The entity of the owner's scene its placed sounds are heard
    /// from, or none for its scene's origin.
    Listen(Option<u32>),
}

/// Why samples were not appended to a clip.
#[derive(Clone, PartialEq, Debug)]
pub enum AppendError {
    /// Past a budget for now; the same samples may come later.
    Full,
    Refused(String),
}

/// A clip a plugin holds, as its ledger counts it.
#[derive(Clone, Copy, Debug)]
struct Held {
    /// Bytes of its samples still counted.
    bytes: u64,
    channels: u16,
    filling: bool,
    once: bool,
    /// Played once, by a sound.
    played: bool,
    /// Played once, to its end.
    spent: bool,
}

/// One plugin's clips and sounds, counted against its [`AudioCaps`].
#[derive(Debug)]
pub struct Ledger {
    /// When the numbering began; sounds of another numbering are
    /// stopped first.
    pub since: Instant,
    next: u32,
    clips: HashMap<u32, Held>,
    /// Clips let go of while a sound still plays them, counted until
    /// none does.
    gone: HashMap<u32, Held>,
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

    /// The audio memory switched on that `bytes` more would pass.
    fn past(&self, bytes: u64) -> Option<u64> {
        self.caps
            .clip_bytes
            .filter(|most| self.bytes.saturating_add(bytes) > *most)
    }

    /// Holds `clip`, whole or to be filled, numbered, if it may and
    /// it fits; why not otherwise.
    pub fn make_clip(&mut self, clip: Clip) -> Result<u32, String> {
        self.may()?;
        let bytes = clip.bytes();
        if let Some(most) = self.past(bytes) {
            return Err(format!(
                "a plugin's clips hold {most} bytes at most"
            ));
        }
        self.bytes = self.bytes.saturating_add(bytes);
        let id = self.number();
        self.clips.insert(
            id,
            Held {
                bytes,
                channels: clip.channels(),
                filling: clip.filling(),
                once: clip.once(),
                played: false,
                spent: false,
            },
        );
        self.changes.push(AudioChange::Clip { id, clip });
        Ok(id)
    }

    /// Adds `samples` at the end of the clip `id`, if it may, it is
    /// filled and they fit.
    pub fn append(
        &mut self,
        id: u32,
        samples: Vec<f32>,
    ) -> Result<(), AppendError> {
        self.may().map_err(AppendError::Refused)?;
        let Some(held) = self.clips.get(&id).copied() else {
            return Err(AppendError::Refused("no such clip".into()));
        };
        if !held.filling {
            return Err(AppendError::Refused(
                "the clip is closed".into(),
            ));
        }
        if held.spent {
            return Err(AppendError::Refused(
                "the clip's sound played to its end".into(),
            ));
        }
        let samples = Clip::checked(samples, held.channels)
            .map_err(AppendError::Refused)?;
        let bytes = size_of_val(samples.as_slice()) as u64;
        if self.past(bytes).is_some() {
            return Err(AppendError::Full);
        }
        self.bytes = self.bytes.saturating_add(bytes);
        if let Some(held) = self.clips.get_mut(&id) {
            held.bytes = held.bytes.saturating_add(bytes);
        }
        self.changes.push(AudioChange::Append { id, samples });
        Ok(())
    }

    /// Ends the filling of the clip `id`.
    pub fn close(&mut self, id: u32) {
        if let Some(held) = self.clips.get_mut(&id)
            && held.filling
        {
            held.filling = false;
            self.changes.push(AudioChange::Close(id));
        }
    }

    /// Lets go of the clip `id`, which ends its filling. Its bytes
    /// stay counted while a sound plays it.
    pub fn drop_clip(&mut self, id: u32) {
        let Some(held) = self.clips.remove(&id) else {
            return;
        };
        if self.sounds.values().any(|clip| *clip == id) {
            self.gone.insert(id, held);
        } else {
            self.bytes -= held.bytes;
        }
        self.changes.push(AudioChange::DropClip(id));
    }

    /// Whether `id` is a clip it holds.
    pub fn holds(&self, id: u32) -> bool {
        self.clips.contains_key(&id)
    }

    /// Whether `id` is a sound playing, as the world last told.
    pub fn playing(&self, id: u32) -> bool {
        self.sounds.contains_key(&id)
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
        if let Some(held) = self.clips.get(&clip)
            && held.once
            && (held.played || playback.looped)
        {
            return Err("a clip filled to be played once is played \
                        once, not looped"
                .into());
        }
        if let Some(held) = self.clips.get_mut(&clip) {
            held.played = true;
        }
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

    /// Moves the volume of the sound `id` to `volume` over `seconds`.
    pub fn fade(&mut self, id: u32, volume: f32, seconds: f32) {
        if self.sounds.contains_key(&id) {
            self.changes.push(AudioChange::Fade {
                id,
                volume,
                seconds,
            });
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

    /// Stops counting the bytes of the clips played once that were
    /// heard: each clip's number and its bytes heard.
    pub fn heard(
        &mut self,
        heard: impl IntoIterator<Item = (u32, u64)>,
    ) {
        for (clip, bytes) in heard {
            self.free(clip, bytes);
        }
    }

    /// Stops counting up to `bytes` of the clip `clip`.
    fn free(&mut self, clip: u32, bytes: u64) {
        let Some(held) = self
            .clips
            .get_mut(&clip)
            .or_else(|| self.gone.get_mut(&clip))
        else {
            return;
        };
        let bytes = bytes.min(held.bytes);
        held.bytes -= bytes;
        self.bytes -= bytes;
    }

    /// Forgets the sound `id`; frees its clip if it was played once,
    /// or if it was let go of and no other sound plays it: whether
    /// it was playing.
    fn end(&mut self, id: u32) -> bool {
        let Some(clip) = self.sounds.remove(&id) else {
            return false;
        };
        if let Some(held) = self.clips.get_mut(&clip)
            && held.once
        {
            held.spent = true;
            self.free(clip, u64::MAX);
        }
        if !self.sounds.values().any(|other| *other == clip)
            && let Some(held) = self.gone.remove(&clip)
        {
            self.bytes -= held.bytes;
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

    #[test]
    fn a_clip_played_once_is_counted_until_heard() {
        let mut ledger = ledger();
        let open = Clip::open(1, 32_000, true).unwrap();
        let fed = ledger.make_clip(open).unwrap();
        ledger.append(fed, vec![0.0; 600]).unwrap();
        assert_eq!(
            ledger.append(fed, vec![0.0; 500]),
            Err(AppendError::Full)
        );
        let sound = ledger.play(fed, ONCE, Place::Unplaced).unwrap();
        assert!(ledger.play(fed, ONCE, Place::Unplaced).is_err());

        // What was heard makes room for more.
        ledger.heard([(fed, 4 * 200)]);
        ledger.append(fed, vec![0.0; 500]).unwrap();
        assert!(ledger.append(fed, vec![0.0; 200]).is_err());

        // At its end its bytes are let go of, and nothing more comes.
        ledger.ended([sound]);
        assert!(ledger.make_clip(clip(1_000)).is_ok());
        assert!(matches!(
            ledger.append(fed, vec![0.0; 1]),
            Err(AppendError::Refused(_))
        ));
    }
}
