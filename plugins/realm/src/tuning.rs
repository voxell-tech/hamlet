//! The world's tuning: the sections of `assets/world.toml` the realm
//! reads, and `assets/palette.toml`, built into the plugin. What each
//! value means is beside its field, in the module it tunes.

use crate::chief::ChiefSettings;
use crate::ground::DetailSettings;
use crate::house::HouseSettings;
use crate::keeper::Motion;
use crate::land::LandSettings;
use crate::layout::TownSettings;
use crate::moving::CarrySettings;
use crate::palette::Palette;
use crate::pick::PickSettings;
use crate::road::RoadSettings;
use crate::shift::HouseMotion;
use crate::sky::{CameraSettings, Looks, SkySettings};
use crate::yard::YardSettings;

const WORLD: &str = include_str!("../../../assets/world.toml");
const PALETTE: &str = include_str!("../../../assets/palette.toml");

/// The sections of `assets/world.toml` the realm reads. Those of
/// what it does not draw are left out.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct WorldTuning {
    pub looks: Looks,
    pub sky: SkySettings,
    pub camera: CameraSettings,
    pub detail: DetailSettings,
    pub motion: Motion,
    pub house_motion: HouseMotion,
    pub pick: PickSettings,
    pub chief: ChiefSettings,
    pub carry: CarrySettings,
    pub play: Play,
    pub generator: GenSettings,
}

/// What the chief's hand plays with. Only what the realm draws is
/// read.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Play {
    /// The fastest a robot scurries, which a mascot hurrying after
    /// its agent keeps up with.
    pub scurry_fastest: f32,
}

/// Everything the generated world is shaped by.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenSettings {
    pub land: LandSettings,
    pub town: TownSettings,
    pub house: HouseSettings,
    pub yard: YardSettings,
    pub road: RoadSettings,
}

impl WorldTuning {
    /// The world's tuning and its palette as the files built in have
    /// them, or why they cannot be read.
    pub fn read() -> Result<(Self, Palette), String> {
        let world = toml::from_str::<Self>(WORLD)
            .map_err(|why| format!("assets/world.toml: {why}"))?;
        let palette = toml::from_str::<Palette>(PALETTE)
            .map_err(|why| format!("assets/palette.toml: {why}"))?;
        Ok((world, palette))
    }
}
