//! The shared materials and meshes, and the robots' kit built from
//! them.

use bevy::prelude::*;

use crate::chief::mascot;
use crate::generator::flora;
use crate::keeper::Kit;
use crate::palette::Palette;

/// Builds the shared materials and meshes and the robots' kit from
/// the world's palette. Add it after `palette::plugin`.
pub fn plugin(app: &mut App) {
    stock(app.world_mut());
}

/// Builds the shared materials and meshes and the kit in `world`.
pub(crate) fn stock(world: &mut World) {
    let pal = world.get_resource_or_init::<Palette>().clone();
    let (stock, kit) = world.resource_scope(
        |world, mut meshes: Mut<Assets<Mesh>>| {
            let mut materials =
                world.resource_mut::<Assets<StandardMaterial>>();
            let stock = Stock::new(&mut meshes, &mut materials, &pal);
            let kit =
                Kit::new(&mut meshes, &mut materials, &stock, &pal);
            (stock, kit)
        },
    );
    world.insert_resource(stock);
    world.insert_resource(kit);
}

/// The materials nearly everything is drawn with, and the meshes
/// drawn in more than one place, each made once and shared. The
/// colour of matte and brass things is in their vertices.
#[derive(Resource, Clone)]
pub struct Stock {
    pub matte: Handle<StandardMaterial>,
    pub brass: Handle<StandardMaterial>,
    /// Every window pane.
    pub windows: Handle<StandardMaterial>,
    /// Each kind of tree, as [`flora::mesh`] makes it.
    pub trees: Vec<Handle<Mesh>>,
    /// The flame of every mascot.
    pub wick: Handle<Mesh>,
}

impl Stock {
    pub fn new(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        pal: &Palette,
    ) -> Self {
        Self {
            windows: materials.add(StandardMaterial {
                base_color: pal.pane_dark,
                perceptual_roughness: 0.3,
                ..default()
            }),
            matte: materials.add(StandardMaterial {
                perceptual_roughness: 0.92,
                reflectance: 0.18,
                ..default()
            }),
            brass: materials.add(StandardMaterial {
                metallic: 0.75,
                perceptual_roughness: 0.42,
                ..default()
            }),
            trees: (0..flora::KINDS)
                .map(|kind| meshes.add(flora::mesh(pal, kind)))
                .collect(),
            wick: meshes.add(mascot::wick().build()),
        }
    }
}
