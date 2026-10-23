//! A clip: samples handed over as they are to be heard.

use core::num::NonZero;
use core::time::Duration;

use bevy::asset::Asset;
use bevy::audio::{ChannelCount, Decodable, SampleRate, Source};
use bevy::reflect::TypePath;

/// The rates a clip may be made at, in frames a second.
pub const RATES: (u32, u32) = (8_000, 96_000);

/// Samples as floats from -1 to 1, interleaved: one channel, or two
/// with the left first, `rate` frames a second.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct Clip {
    samples: Vec<f32>,
    channels: u16,
    rate: u32,
}

impl Clip {
    /// A clip of `samples`, or why it cannot be one. Each sample is
    /// held from -1 to 1.
    pub fn new(
        mut samples: Vec<f32>,
        channels: u16,
        rate: u32,
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
        if samples.is_empty()
            || !samples.len().is_multiple_of(usize::from(channels))
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
        Ok(Self {
            samples,
            channels,
            rate,
        })
    }

    /// Bytes its samples take, as a plugin's audio memory counts
    /// them.
    pub fn bytes(&self) -> u64 {
        size_of_val(self.samples.as_slice()) as u64
    }

    /// Its length in seconds, played as made.
    pub fn seconds(&self) -> f32 {
        let frames = self.samples.len() / usize::from(self.channels);
        frames as f32 / self.rate as f32
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }
}

/// A [`Clip`] being played.
pub struct Playing {
    left: std::vec::IntoIter<f32>,
    channels: u16,
    rate: u32,
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
        NonZero::<u32>::new(self.rate).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(self.long)
    }
}

impl Decodable for Clip {
    type Decoder = Playing;

    fn decoder(&self) -> Playing {
        Playing {
            // Each playing has its own copy: the output thread takes
            // the samples, and a clip is held to its plugin's bytes.
            left: self.samples.clone().into_iter(),
            channels: self.channels,
            rate: self.rate,
            long: Duration::from_secs_f32(self.seconds()),
        }
    }
}
