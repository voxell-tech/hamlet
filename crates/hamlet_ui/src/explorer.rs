//! A hamlet's explorer: the folder it works on as a tree, after VS
//! Code's, kept up with the disk. The tree is in `tree`, kept up by
//! `sync`, drawn by `view` and pressed by `acts`. Where it is shown
//! is up to whoever places [`panel`].

mod acts;
mod sync;
mod tree;
mod view;

use std::path::Path;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use hamlet_core::agents::Works;

pub(crate) use self::view::panel;

pub fn plugin(app: &mut App) {
    app.add_plugins(sync::plugin);
}

/// Opens every folder of `hamlet`'s explorer down to `path`, and
/// selects it.
pub fn reveal(world: &mut World, hamlet: Entity, path: &Path) {
    let Some(folder) = world
        .get::<Works>(hamlet)
        .and_then(|works| works.0.folder.clone())
    else {
        return;
    };
    sync::open(world, hamlet, &folder);
    let path =
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(mut explorer) =
        world.get_mut::<sync::Explorer>(hamlet)
    {
        explorer.0.reveal(&path);
    }
}
