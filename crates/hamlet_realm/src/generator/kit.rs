//! The meshes and materials everything of a kind shares.

use bevy::light::NotShadowCaster;
use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use bevy::render::settings::WgpuFeatures;
use hamlet_vfx::ground::{self, GroundMaterial, GroundSettings};
use hamlet_vfx::inlay::{InlayExt, InlayMaterial, InlayUniform};
use hamlet_vfx::leaves::{LeafMaterial, LeafView, Leaves};
use hamlet_vfx::patch;
use hamlet_vfx::water::{
    self, WaterMaterial, WaterSettings, WaterUniform,
};

use super::land::Land;
use super::loading::Bed;
use super::{Tally, traffic, yard};
use crate::keeper::KeeperState;
use crate::lantern::lantern_glass;
use crate::palette::Palette;
use crate::sky::SkySettings;
use crate::{Sky, Stock};

/// Handles shared by everything of a kind, so the renderer can batch
/// them.
#[derive(Resource)]
pub(super) struct Kit {
    pub(super) matte: Handle<StandardMaterial>,
    /// The ground's patches: matte, with bumps in its light, raised
    /// to the land by its shader.
    pub(super) ground: Handle<GroundMaterial>,
    pub(super) windows: Handle<StandardMaterial>,
    /// One lantern material per state, in the order of
    /// [`KeeperState::ALL`].
    pub(super) lamps: [Handle<StandardMaterial>; 5],
    pub(super) lamp_glass: Handle<Mesh>,
    /// The wheel of a yard's engine and a ring of its steam.
    pub(super) wheel: Handle<Mesh>,
    pub(super) ring: Handle<Mesh>,
    /// A yard's desk with its rack and stool, and its engine.
    pub(super) desk: Handle<Mesh>,
    pub(super) engine: Handle<Mesh>,
    /// The cart of the roads between towns.
    pub(super) cart: Handle<Mesh>,
    /// The water: the sea, the rivers and every pool.
    pub(super) water: Handle<WaterMaterial>,
    /// The grid each patch of the ground and of the sea is drawn in.
    pub(super) patch: Handle<Mesh>,
    /// One quad, for each patch of the sea far enough off to lie
    /// flat.
    pub(super) flat_patch: Handle<Mesh>,
    pub(super) trees: Vec<Handle<Mesh>>,
    /// The leaves of a tree drawn alone, and of many in one mesh.
    pub(super) leaves: Handle<LeafMaterial>,
    pub(super) still_leaves: Handle<LeafMaterial>,
    pub(super) groove: Handle<InlayMaterial>,
}

pub(super) fn state_index(state: KeeperState) -> usize {
    KeeperState::ALL
        .iter()
        .position(|&s| s == state)
        .unwrap_or(0)
}

pub(super) fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut inlays: ResMut<Assets<InlayMaterial>>,
    mut grounds: ResMut<Assets<GroundMaterial>>,
    mut waters: ResMut<Assets<WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
    land: Res<Land>,
    pal: Res<Palette>,
    stock: Res<Stock>,
    bumps: Res<GroundSettings>,
    flow: Res<WaterSettings>,
    leaves: Res<Leaves>,
    device: Option<Res<RenderDevice>>,
) {
    let ground = grounds.add(ground::material(
        &mut images,
        &bumps,
        land.soil(),
        materials.get(&stock.matte).cloned().unwrap_or_default(),
    ));
    let lamps = KeeperState::ALL.map(|_| {
        materials.add(StandardMaterial {
            base_color: pal.glass_lit,
            perceptual_roughness: 0.4,
            ..default()
        })
    });
    let groove = inlays.add(ExtendedMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.85,
            ..default()
        },
        extension: InlayExt::new(
            InlayUniform::default(),
            pal.bed,
            pal.pewter,
        ),
    });

    // One surface of water about the camera, for the sea, the rivers
    // and every pool: wherever the ground dips under it, it shows. It
    // is drawn in patches, which the loading lays. Its reflection is
    // walked by squares of pixels together where the GPU can share
    // values between them.
    let quads = device.is_some_and(|d| {
        d.features().contains(WgpuFeatures::SUBGROUP)
    });
    let water =
        waters.add(water::material(&mut images, &flow, quads));
    let plane = meshes.add(Plane3d::new(Vec3::Y, Vec2::ONE).mesh());
    let patch = meshes.add(patch::grid(patch::QUADS));
    let flat_patch = meshes.add(patch::grid(1));
    // The deep sea's floor under it, where the ground has yet to
    // come and all about the island.
    commands.spawn((
        Bed,
        Mesh3d(plane),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::LinearRgba(LinearRgba::from_vec3(
                pal.water_shade * 0.5,
            )),
            perceptual_roughness: 0.92,
            reflectance: 0.18,
            ..default()
        })),
        Transform::from_xyz(0.0, land.set.land.water_y, 0.0),
        NotShadowCaster,
    ));

    commands.init_resource::<Tally>();

    commands.insert_resource(Kit {
        matte: stock.matte.clone(),
        ground,
        windows: stock.windows.clone(),
        lamps,
        lamp_glass: meshes.add(lantern_glass(0.075, 0.17)),
        wheel: meshes.add(yard::wheel(&land.pal).build()),
        ring: meshes.add(yard::ring().build()),
        desk: meshes
            .add(yard::desk(&land.pal, &land.set.yard).build()),
        engine: meshes
            .add(yard::engine(&land.pal, &land.set.yard).build()),
        cart: meshes.add(traffic::cart(&land.pal).build()),
        water,
        patch,
        flat_patch,
        trees: stock.trees.clone(),
        leaves: leaves.sway.clone(),
        still_leaves: leaves.still.clone(),
        groove,
    });
}

/// Paints the ground as the land has it, and the water as its
/// settings and the sky have it, keeping the wind and the time.
pub(super) fn repaint(
    land: Res<Land>,
    flow: Res<WaterSettings>,
    sky: Res<Sky>,
    kit: Res<Kit>,
    mut grounds: ResMut<Assets<GroundMaterial>>,
    mut waters: ResMut<Assets<WaterMaterial>>,
) {
    let soil = land.soil();
    if let Some(mut ground) = grounds.get_mut(&kit.ground)
        && ground.extension.soil != soil
    {
        ground.extension.soil = soil;
    }
    let Some(now) =
        waters.get(&kit.water).map(|w| w.extension.water.clone())
    else {
        return;
    };
    let look = WaterUniform {
        time: now.time,
        wind: now.wind,
        sky: LinearRgba::from(sky.look.haze)
            .to_vec3()
            .extend(flow.mirror),
        ..WaterUniform::new(&flow)
    };
    if let Some(mut water) = waters.get_mut(&kit.water)
        && water.extension.water != look
    {
        water.extension.water = look;
    }
}

/// Tells the leaves how far into the night it is.
pub(super) fn tell_leaves(
    sky: Res<Sky>,
    set: Res<SkySettings>,
    mut leaves: ResMut<LeafView>,
) {
    leaves.set_if_neq(LeafView {
        night: 1.0 - set.daylight(sky.hour),
    });
}
