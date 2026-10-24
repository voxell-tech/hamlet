//! A sound as samples made in code, and the bank of them.

use std::num::NonZero;
use std::time::Duration;

use bevy::audio::{ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;
use bevy::tasks::futures_lite::future::poll_once;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};

use super::{RATE, Sound};

/// Samples made in code: one channel, or two with the left first.
#[derive(Asset, TypePath, Clone)]
pub struct Clip {
    pub(super) samples: Vec<f32>,
    pub(super) channels: u16,
}

/// A [`Clip`] being played.
pub struct Playing {
    left: vec::IntoIter<f32>,
    channels: u16,
    long: Duration,
}

impl Iterator for Playing {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        self.left.next()
    }
}

impl Source for Playing {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::<u16>::new(self.channels)
            .unwrap_or(NonZero::<u16>::MIN)
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::<u32>::new(RATE).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(self.long)
    }
}

impl Decodable for Clip {
    type Decoder = Playing;

    fn decoder(&self) -> Playing {
        let frames =
            self.samples.len() / self.channels.max(1) as usize;
        Playing {
            // Each playing has its own copy: a clip is a few
            // thousand samples, and a loop or a breath of the music
            // is started once.
            left: self.samples.clone().into_iter(),
            channels: self.channels,
            long: Duration::from_secs_f32(
                frames as f32 / RATE as f32,
            ),
        }
    }
}

/// Every clip, once made.
#[derive(Resource)]
pub(super) struct Bank(Vec<(Sound, Handle<Clip>)>);

impl Bank {
    pub(super) fn clip(&self, sound: Sound) -> Option<Handle<Clip>> {
        self.0
            .iter()
            .find(|(s, _)| *s == sound)
            .map(|(_, clip)| clip.clone())
    }
}

/// The clips being made in the background.
#[derive(Resource)]
pub(super) struct Making(Task<Vec<(Sound, Vec<f32>)>>);

/// Makes the clips in the background the first time they are wanted.
pub(super) fn make_clips(
    mut commands: Commands,
    bank: Option<Res<Bank>>,
    making: Option<Res<Making>>,
) {
    if bank.is_some() || making.is_some() {
        return;
    }
    let task = AsyncComputeTaskPool::get().spawn(async {
        Sound::ALL
            .into_iter()
            .map(|sound| (sound, sound.samples()))
            .collect::<Vec<_>>()
    });
    commands.insert_resource(Making(task));
}

pub(super) fn bank_clips(
    mut commands: Commands,
    mut making: ResMut<Making>,
    mut clips: ResMut<Assets<Clip>>,
) {
    let Some(made) = block_on(poll_once(&mut making.0)) else {
        return;
    };
    let bank = made
        .into_iter()
        .map(|(sound, samples)| {
            let clip = Clip {
                samples,
                channels: 1,
            };
            (sound, clips.add(clip))
        })
        .collect::<Vec<_>>();
    commands.insert_resource(Bank(bank));
    commands.remove_resource::<Making>();
}
