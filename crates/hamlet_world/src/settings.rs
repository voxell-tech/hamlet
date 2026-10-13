//! The sections of `assets/world.toml`. Each is the settings of one
//! module, whose `Default` is the file's value.

use bevy::prelude::*;
use hamlet_assets::{File, Tuning, designed, follow};

use crate::bubble::BubbleSettings;
use crate::chief::ChiefSettings;
use crate::debug::DebugSettings;
use crate::generator::house::HouseSettings;
use crate::generator::land::LandSettings;
use crate::generator::layout::TownSettings;
use crate::generator::road::RoadSettings;
use crate::generator::yard::YardSettings;
use crate::generator::{DetailSettings, GenSettings, HouseMotion};
use crate::hand::PlaySettings;
use crate::keeper::Motion;
use crate::moving::CarrySettings;
use crate::needs::TagSettings;
use crate::pick::PickSettings;
use crate::sky::{CameraSettings, Looks, SkySettings};
use crate::sound::SoundSettings;
use crate::works::WorksSettings;

/// Every tuning value of the world, as `assets/world.toml` has them.
#[derive(Asset, TypePath, Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorldTuning {
    pub looks: Looks,
    pub sky: SkySettings,
    pub camera: CameraSettings,
    pub detail: DetailSettings,
    pub carry: CarrySettings,
    pub motion: Motion,
    pub house_motion: HouseMotion,
    pub pick: PickSettings,
    pub tags: TagSettings,
    pub bubbles: BubbleSettings,
    pub chief: ChiefSettings,
    pub play: PlaySettings,
    pub works: WorksSettings,
    pub sound: SoundSettings,
    pub debug: DebugSettings,
    pub generator: GenSettings,
}

impl Tuning for WorldTuning {
    const FILE: File = hamlet_assets::WORLD;

    /// The generator's settings are not followed: the land keeps the
    /// copy it was built from.
    fn apply(&self, was: &Self, world: &mut World) {
        follow(world, &was.looks, &self.looks);
        follow(world, &was.sky, &self.sky);
        follow(world, &was.camera, &self.camera);
        follow(world, &was.detail, &self.detail);
        follow(world, &was.carry, &self.carry);
        follow(world, &was.motion, &self.motion);
        follow(world, &was.house_motion, &self.house_motion);
        follow(world, &was.pick, &self.pick);
        follow(world, &was.tags, &self.tags);
        follow(world, &was.bubbles, &self.bubbles);
        follow(world, &was.chief, &self.chief);
        follow(world, &was.play, &self.play);
        follow(world, &was.works, &self.works);
        follow(world, &was.sound, &self.sound);
        follow(world, &was.debug, &self.debug);
    }
}

designed!(WorldTuning {
    Looks => looks,
    SkySettings => sky,
    CameraSettings => camera,
    DetailSettings => detail,
    CarrySettings => carry,
    Motion => motion,
    HouseMotion => house_motion,
    PickSettings => pick,
    TagSettings => tags,
    BubbleSettings => bubbles,
    ChiefSettings => chief,
    PlaySettings => play,
    WorksSettings => works,
    SoundSettings => sound,
    DebugSettings => debug,
    GenSettings => generator,
    LandSettings => generator.land,
    TownSettings => generator.town,
    HouseSettings => generator.house,
    YardSettings => generator.yard,
    RoadSettings => generator.road,
});
