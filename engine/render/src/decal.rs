//! Decals of scenes: Bevy's forward decals, a standard material laid
//! on what lies under and over a square. A camera that sees them has
//! a depth prepass.

use bevy::app::App;
use bevy::asset::Assets;
use bevy::ecs::resource::Resource;
use bevy::pbr::StandardMaterial;
use bevy::pbr::decal::{ForwardDecalMaterial, ForwardDecalPlugin};

/// The material of a decal.
pub(crate) type DecalMaterial =
    ForwardDecalMaterial<StandardMaterial>;

pub(crate) fn plugin(app: &mut App) {
    // Without Bevy's decals, as in the harness, a decal is kept all
    // the same and drawn nowhere.
    if app.is_plugin_added::<ForwardDecalPlugin>() {
        app.init_resource::<DrawsDecals>();
    } else {
        app.init_resource::<Assets<DecalMaterial>>();
    }
}

/// Present when Bevy draws decals.
#[derive(Resource, Default, Debug)]
pub(crate) struct DrawsDecals;
