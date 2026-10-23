//! A clip: samples handed over as they are to be heard, whole or a
//! part at a time.

use core::num::NonZero;
use core::time::Duration;

use bevy::asset::Asset;
use bevy::audio::{ChannelCount, Decodable, SampleRate, Source};
use bevy::reflect::TypePath;
use crossbeam_channel::{Receiver, Sender, TryRecvError};

/// The rates a clip may be made at, in frames a second.
pub const RATES: (u32, u32) = (8_000, 96_000);

/// Samples as floats from -1 to 1, interleaved: one channel, or two
/// with the left first, `rate` frames a second.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct Clip {
    samples: Vec<f32>,
    channels: u16,
    rate: u32,
    /// Still filled, with more samples to come.
    filling: bool,
    /// Played by one sound once, its samples let go as they are
    /// heard.
    once: bool,
    /// A clip played once whose samples went to its sound.
    spent: bool,
    /// The samples still to come, for a sound that plays the clip
    /// while it is filled.
    feed: Option<Receiver<Vec<f32>>>,
    /// Played again from the start, by its own [`Playing`].
    looped: bool,
}

impl Clip {
    /// A clip of `samples`, or why it cannot be one. Each sample is
    /// held from -1 to 1.
    pub fn new(
        samples: Vec<f32>,
        channels: u16,
        rate: u32,
    ) -> Result<Self, String> {
        let mut clip = Self::open(channels, rate, false)?;
        if samples.is_empty() {
            return Err("a clip has samples".into());
        }
        clip.samples = Self::checked(samples, channels)?;
        clip.filling = false;
        Ok(clip)
    }

    /// An empty clip to be filled, played `once` or not, or why it
    /// cannot be one.
    pub fn open(
        channels: u16,
        rate: u32,
        once: bool,
    ) -> Result<Self, String> {
        if !(1..=2).contains(&channels) {
            return Err(format!(
                "a clip has 1 or 2 channels, not {channels}"
            ));
        }
        if !(RATES.0..=RATES.1).contains(&rate) {
            return Err(format!(
                "a clip's rate is {} to {} frames a second, not \
                 {rate}",
                RATES.0, RATES.1
            ));
        }
        Ok(Self {
            samples: Vec::new(),
            channels,
            rate,
            filling: true,
            once,
            spent: false,
            feed: None,
            looped: false,
        })
    }

    /// `samples` as a clip of `channels` holds them, each from -1 to
    /// 1, or why they cannot be.
    pub fn checked(
        mut samples: Vec<f32>,
        channels: u16,
    ) -> Result<Vec<f32>, String> {
        if !samples.len().is_multiple_of(usize::from(channels.max(1)))
        {
            return Err(format!(
                "{} samples are not whole frames of {channels} \
                 channels",
                samples.len()
            ));
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err("a sample is not a finite number".into());
        }
        for sample in &mut samples {
            *sample = sample.clamp(-1.0, 1.0);
        }
        Ok(samples)
    }

    /// Bytes its samples take, as a plugin's audio memory counts
    /// them.
    pub fn bytes(&self) -> u64 {
        size_of_val(self.samples.as_slice()) as u64
    }

    /// Its length in seconds, played as made.
    pub fn seconds(&self) -> f32 {
        self.seconds_of(self.samples.len())
    }

    /// The seconds `samples` of it last, played as made.
    pub fn seconds_of(&self, samples: usize) -> f32 {
        let frames = samples / usize::from(self.channels);
        frames as f32 / self.rate as f32
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn filling(&self) -> bool {
        self.filling
    }

    pub fn once(&self) -> bool {
        self.once
    }

    /// Adds `samples`, checked, at its end. A clip played once keeps
    /// none once its sound has them.
    pub(crate) fn append(&mut self, samples: &[f32]) {
        if !self.spent {
            self.samples.extend_from_slice(samples);
        }
    }

    pub(crate) fn close(&mut self) {
        self.filling = false;
    }

    /// A clip of its samples so far for one sound to play, `looped`
    /// or not, and where the rest are sent. A clip played once gives
    /// it its samples.
    pub(crate) fn fed(
        &mut self,
        looped: bool,
    ) -> (Self, Sender<Vec<f32>>) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let samples = if self.once {
            // Sent rather than kept, so that the output thread holds
            // the only copy.
            self.spent = true;
            let _ = sender.send(core::mem::take(&mut self.samples));
            Vec::new()
        } else {
            self.samples.clone()
        };
        let played = Self {
            samples,
            channels: self.channels,
            rate: self.rate,
            filling: true,
            once: self.once,
            spent: false,
            feed: Some(receiver),
            looped,
        };
        (played, sender)
    }
}

/// A [`Clip`] being played.
pub struct Playing {
    samples: Vec<f32>,
    at: usize,
    feed: Option<Receiver<Vec<f32>>>,
    channels: u16,
    rate: u32,
    once: bool,
    looped: bool,
    /// Samples of silence left of a frame played while waiting for
    /// more.
    silent: u16,
}

impl Iterator for Playing {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        loop {
            if self.silent > 0 {
                self.silent -= 1;
                return Some(0.0);
            }
            if let Some(sample) = self.samples.get(self.at) {
                self.at += 1;
                return Some(*sample);
            }
            let Some(feed) = &self.feed else {
                if self.looped && self.at > 0 {
                    self.at = 0;
                    continue;
                }
                return None;
            };
            match feed.try_recv() {
                // Played once, what was heard is let go of.
                Ok(more) if self.once => {
                    self.samples = more;
                    self.at = 0;
                }
                Ok(more) => self.samples.extend(more),
                Err(TryRecvError::Empty) => {
                    self.silent = self.channels
                }
                Err(TryRecvError::Disconnected) => self.feed = None,
            }
        }
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
        NonZero::<u32>::new(self.rate).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        if self.feed.is_some() || self.looped {
            return None;
        }
        let frames = self.samples.len() / usize::from(self.channels);
        Some(Duration::from_secs_f32(
            frames as f32 / self.rate as f32,
        ))
    }
}

impl Decodable for Clip {
    type Decoder = Playing;

    fn decoder(&self) -> Playing {
        Playing {
            // Each playing has its own copy: the output thread takes
            // the samples, and a clip is held to its plugin's bytes.
            samples: self.samples.clone(),
            at: 0,
            feed: self.feed.clone(),
            channels: self.channels,
            rate: self.rate,
            once: self.once,
            looped: self.looped,
            silent: 0,
        }
    }
}
