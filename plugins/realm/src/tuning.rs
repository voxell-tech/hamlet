//! The world's tuning: the sections of `assets/world.toml` the realm
//! reads, `assets/palette.toml` and `assets/vfx.toml`, built into the
//! plugin. What each value means is beside its field, in the module
//! it tunes.

use hamlet_engine_bevy::prelude::{Resource, bevy_ecs};

use crate::folk::chief::ChiefSettings;
use crate::folk::hand::PlaySettings;
use crate::folk::keeper::Motion;
use crate::folk::talk::BalloonSettings;
use crate::fx::debug::DebugSettings;
use crate::fx::magic::MagicSettings;
use crate::fx::sparks::SparkSettings;
use crate::fx::wind::WindSettings;
use crate::places::house::HouseSettings;
use crate::places::moving::CarrySettings;
use crate::places::road::RoadSettings;
use crate::places::shift::HouseMotion;
use crate::places::town::works::WorksSettings;
use crate::places::yard::YardSettings;
use crate::view::pick::PickSettings;
use crate::world::ground::DetailSettings;
use crate::world::land::LandSettings;
use crate::world::layout::TownSettings;
use crate::world::palette::Palette;
use crate::world::sky::{CameraSettings, Looks, SkySettings};

const WORLD: &str = include_str!("../../../assets/world.toml");
const PALETTE: &str = include_str!("../../../assets/palette.toml");
const VFX: &str = include_str!("../../../assets/vfx.toml");
const UI: &str = include_str!("../../../assets/ui.toml");

/// The sections of `assets/world.toml` the realm reads. Those of
/// what it does not draw are left out.
#[derive(Resource, Clone, Debug, serde::Deserialize)]
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
    pub play: PlaySettings,
    pub bubbles: BalloonSettings,
    pub works: WorksSettings,
    pub debug: DebugSettings,
    pub generator: GenSettings,
    pub tags: TagSettings,
    /// Seconds the pointer rests on a thing before its card shows,
    /// from `[feel]` of `assets/ui.toml`.
    #[serde(skip)]
    pub hover_wait: f32,
}

/// The paper tags over the houses of agents that wait for the chief.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
pub struct TagSettings {
    /// Most characters a tag says of what waits.
    pub line: usize,
    /// Widest a tag gets, in pixels.
    pub width: f32,
    /// Height a tag hangs above the roof, in pixels.
    pub lift: f32,
}

/// The one value of `[feel]` the hover cards read.
#[derive(serde::Deserialize)]
struct Feel {
    feel: Waits,
}

#[derive(serde::Deserialize)]
struct Waits {
    hover_wait: f32,
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

/// The sections of `assets/vfx.toml` the realm's effects read. The
/// flag's is read by the towns.
#[derive(Resource, Clone, Debug, serde::Deserialize)]
pub struct VfxTuning {
    pub sparks: SparkSettings,
    pub magic: MagicSettings,
    pub wind: WindSettings,
    /// Radius of the room about a thinking agent's light that a
    /// pointer can hit, from the play settings.
    #[serde(skip)]
    pub nudge_room: f32,
}

impl VfxTuning {
    /// The effects' tuning as the files built in have it, or why it
    /// cannot be read.
    pub fn read() -> Result<Self, String> {
        let mut vfx = toml::from_str::<Self>(VFX)
            .map_err(|why| format!("assets/vfx.toml: {why}"))?;
        vfx.nudge_room = WorldTuning::read()?.0.play.nudge_room;
        Ok(vfx)
    }
}

impl WorldTuning {
    /// The world's tuning and its palette as the files built in have
    /// them, or why they cannot be read.
    pub fn read() -> Result<(Self, Palette), String> {
        let mut world = toml::from_str::<Self>(WORLD)
            .map_err(|why| format!("assets/world.toml: {why}"))?;
        world.hover_wait = toml::from_str::<Feel>(UI)
            .map_err(|why| format!("assets/ui.toml: {why}"))?
            .feel
            .hover_wait;
        let palette = toml::from_str::<Palette>(PALETTE)
            .map_err(|why| format!("assets/palette.toml: {why}"))?;
        Ok((world, palette))
    }
}
