//! The meshes and materials everything of a kind shares.

use bevy::light::NotShadowCaster;
use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;
use hamlet_vfx::ground::{self, GroundMaterial, GroundSettings};
use hamlet_vfx::inlay::{InlayExt, InlayMaterial, InlayUniform};
use hamlet_vfx::water::{
    self, WaterMaterial, WaterSettings, WaterUniform, Waves,
};

use super::land::Land;
use super::loading::{Bed, Sea};
use super::{Tally, flora, traffic, yard};
use crate::Stock;
use crate::keeper::KeeperState;
use crate::lantern::lantern_glass;
use crate::palette::Palette;

/// Handles shared by everything of a kind, so the renderer can batch
/// them.
#[derive(Resource)]
pub(super) struct Kit {
    pub(super) matte: Handle<StandardMaterial>,
    /// The tiles of ground: matte, with bumps in its light.
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
    pub(super) trees: Vec<Handle<Mesh>>,
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

    // One surface of water under the camera, for the sea, the rivers
    // and every pool: wherever the ground dips under it, it shows.
    let water = waters
        .add(water::material(paint(&pal, &flow), flow.roughness));
    let plane = meshes.add(Plane3d::new(Vec3::Y, Vec2::ONE).mesh());
    commands.spawn((
        Sea,
        Waves,
        Mesh3d(plane.clone()),
        MeshMaterial3d(water.clone()),
        Transform::from_xyz(0.0, land.set.land.water_y, 0.0),
        NotShadowCaster,
    ));
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

    let kinds = (0..flora::KINDS)
        .map(|k| meshes.add(flora::mesh(&land.pal, k)))
        .collect::<Vec<_>>();
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
        trees: kinds,
        groove,
    });
}

/// The water's look in the palette's colours.
fn paint(pal: &Palette, set: &WaterSettings) -> WaterUniform {
    WaterUniform::new(set, pal.water, pal.water_shade, pal.foam)
}

/// Paints the ground and the water as the land and the water's
/// settings now have them.
pub(super) fn repaint(
    land: Res<Land>,
    flow: Res<WaterSettings>,
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
    let look = paint(&land.pal, &flow);
    if let Some(mut water) = waters.get_mut(&kit.water)
        && (water.extension.water != look
            || water.base.perceptual_roughness != flow.roughness)
    {
        water.extension.water = look;
        water.base.perceptual_roughness = flow.roughness;
    }
}
