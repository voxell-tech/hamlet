//! The world's tuning: the sections of `assets/world.toml` the realm
//! reads, `assets/palette.toml`, and the realm's own
//! `plugins/realm/assets/bazaar.toml` and `looks.toml`, built into
//! the plugin. What
//! each value means is beside its field, in the module it tunes.

use crate::folk::chief::ChiefSettings;
use crate::folk::keeper::Motion;
use crate::folk::keeper::wear::WearSettings;
use crate::places::bazaar::BazaarSettings;
use crate::places::house::HouseSettings;
use crate::places::moving::CarrySettings;
use crate::places::road::RoadSettings;
use crate::places::shift::HouseMotion;
use crate::places::town::attire::AttireSettings;
use crate::places::yard::YardSettings;
use crate::view::pick::PickSettings;
use crate::wardrobe::closet::ClosetSettings;
use crate::world::ground::DetailSettings;
use crate::world::land::LandSettings;
use crate::world::layout::TownSettings;
use crate::world::palette::{BazaarColours, Palette};
use crate::world::sky::{CameraSettings, Looks, SkySettings};

const WORLD: &str = include_str!("../../../assets/world.toml");
const PALETTE: &str = include_str!("../../../assets/palette.toml");
const BAZAAR: &str = include_str!("../assets/bazaar.toml");
const LOOKS: &str = include_str!("../assets/looks.toml");

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
    /// Read from the bazaar's own file.
    #[serde(skip)]
    pub bazaar: BazaarSettings,
    /// Read from the looks' own file.
    #[serde(skip)]
    pub closet: ClosetSettings,
    #[serde(skip)]
    pub wear: WearSettings,
    #[serde(skip)]
    pub attire: AttireSettings,
}

/// The looks' own file.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LooksTuning {
    closet: ClosetSettings,
    wear: WearSettings,
    attire: AttireSettings,
}

/// The bazaar's own file.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BazaarTuning {
    bazaar: BazaarSettings,
    palette: BazaarColours,
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
        let mut world = toml::from_str::<Self>(WORLD)
            .map_err(|why| format!("assets/world.toml: {why}"))?;
        let mut palette = toml::from_str::<Palette>(PALETTE)
            .map_err(|why| format!("assets/palette.toml: {why}"))?;
        let bazaar = toml::from_str::<BazaarTuning>(BAZAAR).map_err(
            |why| format!("plugins/realm/assets/bazaar.toml: {why}"),
        )?;
        world.bazaar = bazaar.bazaar;
        palette.bazaar = bazaar.palette;
        let looks =
            toml::from_str::<LooksTuning>(LOOKS).map_err(|why| {
                format!("plugins/realm/assets/looks.toml: {why}")
            })?;
        world.closet = looks.closet;
        world.wear = looks.wear;
        world.attire = looks.attire;
        Ok((world, palette))
    }
}
