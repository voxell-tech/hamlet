//! The chief's choices of how the world is drawn: its spending on
//! looking good, and the hour its sky keeps.

use bevy::ecs::resource::Resource;

/// The world's spending on looking good.
#[derive(
    Resource,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    /// No particles, no bloom, plain edges and small shadows.
    Low,
    /// Fewer particles and middling shadows.
    Medium,
    #[default]
    High,
}

impl Quality {
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
        }
    }

    /// The share of the full number of particles it draws.
    pub fn density(self) -> f32 {
        match self {
            Self::Low => 0.0,
            Self::Medium => 0.4,
            Self::High => 1.0,
        }
    }

    /// The share of the full shadow map's side it uses.
    pub fn shadows(self) -> f32 {
        match self {
            Self::Low => 0.25,
            Self::Medium => 0.5,
            Self::High => 1.0,
        }
    }
}

/// The sky following the clock, or held at day or night.
#[derive(
    Resource,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Debug,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum TimeOfDay {
    #[default]
    Clock,
    Day,
    Night,
}
