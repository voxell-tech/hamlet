//! The clips of every sound, made in code the first time the realm
//! is heard, a few each frame so that making them is not seen.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use hamlet_engine_bevy::audio::Sound as Clip;
use hamlet_engine_bevy::prelude::{
    Handle, Res, ResMut, Resource, SoundAssets, bevy_ecs,
};

use crate::sound::kinds::Sound;
use crate::sound::mix::Hearing;
use crate::sound::synth::{self, RATE};

/// The longest a frame spends making clips, past the first it makes.
const MAKING: Duration = Duration::from_millis(4);

/// Every clip made so far, and how many of [`Sound::ALL`] were
/// tried.
#[derive(Resource, Default)]
pub struct Bank {
    clips: HashMap<Sound, Handle<Clip>>,
    tried: usize,
}

impl Bank {
    pub fn clip(&self, sound: Sound) -> Option<Handle<Clip>> {
        self.clips.get(&sound).cloned()
    }
}

/// Makes the clips left to make, once the realm is heard.
pub fn make_clips(
    hearing: Res<Hearing>,
    mut bank: ResMut<Bank>,
    mut assets: ResMut<SoundAssets>,
) {
    if !hearing.0 || bank.tried == Sound::ALL.len() {
        return;
    }
    let until = Instant::now() + MAKING;
    while let Some(&sound) = Sound::ALL.get(bank.tried) {
        bank.tried += 1;
        match Clip::new(&synth::make(sound), 1, RATE) {
            Ok(clip) => {
                let clip = assets.add(clip);
                bank.clips.insert(sound, clip);
            }
            Err(why) => hamlet_engine_bevy::warn(&format!(
                "a sound of the realm is not made: {why}"
            )),
        }
        if Instant::now() >= until {
            break;
        }
    }
}
