//! Numbers that depend on where they are read.

use bevy_math::Vec3;
use fastnoise_lite::{
    CellularReturnType, FastNoiseLite, FractalType, NoiseType,
};
use serde::{Deserialize, Serialize};

use crate::seed::Seed;
use crate::value::Placement;

/// A number read at each placement.
// Boxed: a field is a tree of the fields its nodes were given.
#[derive(Clone, PartialEq, Debug)]
pub enum Field {
    Constant(f32),
    Noise(Noise),
    Measure(Measure),
    Remap {
        field: Box<Field>,
        from: [f32; 2],
        to: [f32; 2],
    },
    Add(Box<Field>, Box<Field>),
    Multiply(Box<Field>, Box<Field>),
}

impl Field {
    pub fn sample(&self, placement: &Placement) -> f32 {
        match self {
            Field::Constant(value) => *value,
            Field::Noise(noise) => {
                noise.sample(placement.frame.translation)
            }
            Field::Measure(measure) => measure.of(placement),
            Field::Remap { field, from, to } => {
                let span = from[1] - from[0];
                let t = if span == 0.0 {
                    0.0
                } else {
                    (field.sample(placement) - from[0]) / span
                };
                to[0] + (to[1] - to[0]) * t
            }
            Field::Add(a, b) => {
                a.sample(placement) + b.sample(placement)
            }
            Field::Multiply(a, b) => {
                a.sample(placement) * b.sample(placement)
            }
        }
    }
}

/// A fact of the placement itself.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum Measure {
    X,
    Height,
    Z,
    Along,
    Index,
}

impl Measure {
    fn of(self, placement: &Placement) -> f32 {
        match self {
            Measure::X => placement.frame.translation.x,
            Measure::Height => placement.frame.translation.y,
            Measure::Z => placement.frame.translation.z,
            Measure::Along => placement.along,
            Measure::Index => placement.index as f32,
        }
    }
}

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum NoiseKind {
    Value,
    Gradient,
    Simplex,
    VoronoiDistance,
    VoronoiEdges,
    VoronoiCell,
}

/// Noise sampled in world space, from 0 to 1.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Noise {
    pub kind: NoiseKind,
    /// Features per metre.
    pub frequency: f32,
    pub octaves: u8,
    pub seed: Seed,
}

impl Noise {
    pub fn sample(&self, at: Vec3) -> f32 {
        let mut noise = FastNoiseLite::with_seed(self.seed.0 as i32);
        noise.set_frequency(Some(self.frequency));
        let (kind, cellular) = match self.kind {
            NoiseKind::Value => (NoiseType::ValueCubic, None),
            NoiseKind::Gradient => (NoiseType::Perlin, None),
            NoiseKind::Simplex => (NoiseType::OpenSimplex2, None),
            NoiseKind::VoronoiDistance => (
                NoiseType::Cellular,
                Some(CellularReturnType::Distance),
            ),
            NoiseKind::VoronoiEdges => (
                NoiseType::Cellular,
                Some(CellularReturnType::Distance2Sub),
            ),
            NoiseKind::VoronoiCell => (
                NoiseType::Cellular,
                Some(CellularReturnType::CellValue),
            ),
        };
        noise.set_noise_type(Some(kind));
        noise.set_cellular_return_type(cellular);
        if self.octaves > 1 {
            noise.set_fractal_type(Some(FractalType::FBm));
            noise.set_fractal_octaves(Some(i32::from(self.octaves)));
        }
        (noise.get_noise_3d(at.x, at.y, at.z) * 0.5 + 0.5)
            .clamp(0.0, 1.0)
    }
}
