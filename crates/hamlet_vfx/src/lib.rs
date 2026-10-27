//! The world's visual effects: what each one is and how it plays.
//! When and where one is set off is the world's business.

pub mod flag;
pub mod ground;
pub mod inlay;
pub mod magic;
pub mod mark;
pub mod sparks;
pub mod wind;

use bevy::prelude::*;
use hamlet_assets::{File, Tuning, TuningPlugin, designed, follow};

use self::flag::FlagSettings;
use self::ground::GroundSettings;
use self::inlay::InlayPlugin;
use self::magic::MagicSettings;
use self::sparks::SparkSettings;
use self::wind::WindSettings;

/// Every effect. Insert [`Tints`] before adding it.
pub struct VfxPlugin;

impl Plugin for VfxPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<InlayPlugin>() {
            app.add_plugins(InlayPlugin);
        }
        app.add_plugins((
            TuningPlugin::<VfxTuning>::default(),
            sparks::plugin,
            magic::plugin,
            mark::plugin,
            wind::plugin,
            flag::plugin,
            ground::plugin,
        ))
        .init_resource::<Now>();
    }
}

/// The effects' systems. An effect set off before these plays in
/// the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct VfxSystems;

/// The time the effects play against, in seconds. Whoever keeps the
/// scene's clock sets it.
#[derive(Resource, Default, Clone, Copy)]
pub struct Now(pub f32);

/// The share of the particles asked for that are made. At 0 there
/// are none.
#[derive(Resource, Clone, Copy, PartialEq)]
pub struct Density(pub f32);

impl Default for Density {
    fn default() -> Self {
        Self(1.0)
    }
}

/// The colours the effects glow in, in linear RGB.
#[derive(Resource, Clone, Copy, PartialEq)]
pub struct Tints {
    pub ember: Vec3,
    pub firefly: Vec3,
    pub rune: Vec3,
    pub wind: Vec3,
}

/// The effects' tuning, as `assets/vfx.toml` has it.
#[derive(Asset, TypePath, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VfxTuning {
    pub sparks: SparkSettings,
    pub magic: MagicSettings,
    pub wind: WindSettings,
    pub flag: FlagSettings,
    pub ground: GroundSettings,
}

impl Tuning for VfxTuning {
    const FILE: File = hamlet_assets::VFX;

    fn apply(&self, was: &Self, world: &mut World) {
        follow(world, &was.sparks, &self.sparks);
        follow(world, &was.magic, &self.magic);
        follow(world, &was.wind, &self.wind);
        follow(world, &was.flag, &self.flag);
        follow(world, &was.ground, &self.ground);
    }
}

designed!(VfxTuning {
    SparkSettings => sparks,
    MagicSettings => magic,
    WindSettings => wind,
    FlagSettings => flag,
    GroundSettings => ground,
});
