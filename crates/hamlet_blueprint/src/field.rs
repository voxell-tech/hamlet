//! Numbers that depend on where they are read.

use bevy_math::Vec3;
use fastnoise_lite::{
    CellularReturnType, FastNoiseLite, FractalType, NoiseType,
};
use serde::{Deserialize, Serialize};

use crate::seed::Seed;
use crate::value::Spot;

/// A number read at each spot.
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
    Math(Op, Box<Field>, Box<Field>),
    Random(Random),
}

impl Field {
    pub fn sample(&self, spot: &Spot) -> f32 {
        match self {
            Field::Constant(value) => *value,
            Field::Noise(noise) => {
                noise.sample(spot.frame.translation)
            }
            Field::Measure(measure) => measure.of(spot),
            Field::Remap { field, from, to } => {
                let span = from[1] - from[0];
                let t = if span == 0.0 {
                    0.0
                } else {
                    (field.sample(spot) - from[0]) / span
                };
                to[0] + (to[1] - to[0]) * t
            }
            Field::Math(op, a, b) => {
                op.apply(a.sample(spot), b.sample(spot))
            }
            Field::Random(random) => random.sample(spot),
        }
    }
}

/// What a math field works out from its two numbers, `a` and `b`.
/// Trigonometry and powers come from libm and square roots are exact,
/// so a result is the same on every platform.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum Op {
    Add,
    Subtract,
    Multiply,
    Divide,
    Min,
    Max,
    /// `a` to the power `b`.
    Power,
    /// The rest of `a` divided by `b`, as far from zero as `b`.
    Modulo,
    Abs,
    Floor,
    Ceil,
    Round,
    Sqrt,
    /// Sine of `a` degrees.
    Sin,
    Cos,
    Tan,
    Hypot,
    /// The angle in degrees whose rise is `a` over a run of `b`.
    Angle,
    /// 1 where `a` is below `b`, else 0.
    Less,
    /// 1 where `a` is above `b`, else 0.
    Greater,
}

impl Op {
    /// The names math nodes offer, in order, with the op each is.
    pub const ALL: [(&'static str, Op); 20] = [
        ("add", Op::Add),
        ("subtract", Op::Subtract),
        ("multiply", Op::Multiply),
        ("divide", Op::Divide),
        ("min", Op::Min),
        ("max", Op::Max),
        ("power", Op::Power),
        ("modulo", Op::Modulo),
        ("abs", Op::Abs),
        ("floor", Op::Floor),
        ("ceil", Op::Ceil),
        ("round", Op::Round),
        ("sqrt", Op::Sqrt),
        ("sin", Op::Sin),
        ("cos", Op::Cos),
        ("tan", Op::Tan),
        ("hypot", Op::Hypot),
        ("angle", Op::Angle),
        ("less", Op::Less),
        ("greater", Op::Greater),
    ];

    pub fn apply(self, a: f32, b: f32) -> f32 {
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        match self {
            Op::Add => a + b,
            Op::Subtract => a - b,
            Op::Multiply => a * b,
            Op::Divide | Op::Modulo if b == 0.0 => 0.0,
            Op::Divide => a / b,
            Op::Min => a.min(b),
            Op::Max => a.max(b),
            Op::Power => libm::powf(a, b),
            Op::Modulo => a.rem_euclid(b),
            Op::Abs => a.abs(),
            Op::Floor => a.floor(),
            Op::Ceil => a.ceil(),
            Op::Round => a.round(),
            Op::Sqrt => a.max(0.0).sqrt(),
            Op::Sin => libm::sinf(a.to_radians()),
            Op::Cos => libm::cosf(a.to_radians()),
            Op::Tan => libm::tanf(a.to_radians()),
            Op::Hypot => (a * a + b * b).sqrt(),
            Op::Angle => libm::atan2f(a, b).to_degrees(),
            Op::Less => flag(a < b),
            Op::Greater => flag(a > b),
        }
    }
}

/// A number picked at random between two, again for each spot or
/// once for all of them.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Random {
    pub low: f32,
    pub high: f32,
    pub seed: Seed,
    /// Whether each spot rolls its own.
    pub each: bool,
}

impl Random {
    fn sample(&self, spot: &Spot) -> f32 {
        let key = if self.each {
            self.seed.mix(spot.id.0)
        } else {
            self.seed
        };
        self.low + (self.high - self.low) * key.unit()
    }
}

/// A fact of the spot itself.
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
    fn of(self, spot: &Spot) -> f32 {
        match self {
            Measure::X => spot.frame.translation.x,
            Measure::Height => spot.frame.translation.y,
            Measure::Z => spot.frame.translation.z,
            Measure::Along => spot.along,
            Measure::Index => spot.index as f32,
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
