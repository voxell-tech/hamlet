//! The shared materials, and the robots' kit built from them.

use bevy::prelude::*;

use crate::keeper::Kit;
use crate::palette::Palette;

/// The materials nearly everything is drawn with. The colour of
/// matte and brass things is in their vertices.
#[derive(Resource, Clone)]
pub struct Stock {
    pub matte: Handle<StandardMaterial>,
    pub brass: Handle<StandardMaterial>,
    /// Every window pane.
    pub windows: Handle<StandardMaterial>,
}

impl Stock {
    pub fn new(
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
        }
    }
}

/// Builds the shared materials and the robots' kit from the world's
/// palette.
pub(crate) fn stock(world: &mut World) {
    let pal = world.get_resource_or_init::<Palette>().clone();
    let stock = Stock::new(
        &mut world.resource_mut::<Assets<StandardMaterial>>(),
        &pal,
    );
    let kit = world.resource_scope(
        |world, mut meshes: Mut<Assets<Mesh>>| {
            Kit::new(
                &mut meshes,
                &mut world.resource_mut::<Assets<StandardMaterial>>(),
                &stock,
                &pal,
            )
        },
    );
    world.insert_resource(stock);
    world.insert_resource(kit);
}
