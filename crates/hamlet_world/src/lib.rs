//! The 3D world of the command center (docs/tour.md): the generated
//! world and what it stands on.

pub mod bubble;
pub mod chief;
pub mod debug;
pub mod generator;
pub mod hamlet;
pub mod hand;
pub mod keeper;
pub mod lantern;
pub mod library;
pub mod meshes;
pub mod moving;
pub mod needs;
pub mod palette;
pub mod pick;
pub mod play;
pub mod settings;
pub mod sky;
pub mod sound;

mod label;
mod stage;
mod stock;

use bevy::light::DirectionalLightShadowMap;
use bevy::prelude::*;
use hamlet_assets::TuningPlugin;
use hamlet_vfx::inlay::InlayPlugin;
use hamlet_vfx::magic::MagicSettings;

use self::generator::{DetailSettings, HouseMotion};
use self::keeper::Motion;
use self::palette::Palette;
use self::settings::WorldTuning;
use self::sky::{CameraSettings, Looks, Quality, SkySettings};
pub use self::sky::{Dark, DayPart, Hour, LowSun, Sky, WorldCamera};
pub use self::stage::{Clock, Stage};
pub use self::stock::Stock;
pub(crate) use self::stock::stock;

/// The world's systems, in the order they run each frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldSystems;

/// The steps of [`WorldSystems`], in order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Step {
    Clock,
    /// A world's following of the stage: spawning, placing and
    /// lighting.
    Scene,
    /// The robots take their poses.
    Robots,
}

/// The base of every world: the clock, the stage, the palette, the
/// settings, the camera and its lights, the shared materials and the
/// robots.
pub fn base(app: &mut App) {
    app.add_plugins((
        InlayPlugin,
        TuningPlugin::<WorldTuning>::default(),
        TuningPlugin::<Palette>::default(),
        play::plugin,
        sound::plugin,
        debug::plugin,
    ))
    .init_resource::<Clock>()
    .init_resource::<Stage>()
    .init_resource::<Palette>()
    .init_resource::<Looks>()
    .init_resource::<CameraSettings>()
    .init_resource::<DetailSettings>()
    .init_resource::<Motion>()
    .init_resource::<HouseMotion>()
    .init_resource::<MagicSettings>()
    .init_resource::<Quality>()
    .init_resource::<SkySettings>()
    .init_resource::<Hour>()
    .init_state::<DayPart>()
    .add_computed_state::<Dark>()
    .add_computed_state::<LowSun>()
    .configure_sets(
        Update,
        (Step::Clock, Step::Scene, Step::Robots)
            .chain()
            .in_set(WorldSystems),
    )
    .add_systems(Startup, sky::spawn)
    .add_systems(OnEnter(LowSun), sky::raise_haze)
    .add_systems(OnExit(LowSun), sky::clear_haze)
    .add_systems(
        Update,
        (
            (stage::tick, sky::tell).chain().in_set(Step::Clock),
            (
                (sky::light, sky::light_windows)
                    .run_if(resource_changed::<Sky>),
                (sky::occlude, sky::sky_dome)
                    .run_if(resource_changed::<SkySettings>),
                sky::spend.run_if(resource_changed::<Quality>),
                sky::lay_haze.run_if(in_state(LowSun)),
            )
                .in_set(Step::Scene),
            keeper::animate.in_set(Step::Robots),
        ),
    );
    let world = app.world_mut();
    let sky = Sky::at(
        world.resource::<Looks>(),
        world.resource::<SkySettings>(),
        world.resource::<SkySettings>().held(false),
    );
    world.insert_resource(sky);
    let size = world.resource::<CameraSettings>().shadow_map;
    world.insert_resource(DirectionalLightShadowMap { size });
    stock(world);
}

/// Puts `child` under `parent`, or despawns it if the parent has
/// gone, as a house does with its town.
pub(crate) fn adopt(
    commands: &mut Commands,
    parent: Entity,
    child: Entity,
) {
    commands.queue(move |world: &mut World| {
        if world.get_entity(parent).is_ok() {
            if let Ok(mut child) = world.get_entity_mut(child) {
                child.insert(ChildOf(parent));
            }
        } else if let Ok(child) = world.get_entity_mut(child) {
            child.despawn();
        }
    });
}
