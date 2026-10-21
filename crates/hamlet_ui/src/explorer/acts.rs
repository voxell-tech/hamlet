//! What the presses in an explorer do.

use std::path::{Path, PathBuf};

use bevy::clipboard::Clipboard;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use hamlet_desktop::files::Entry;
use hamlet_desktop::open;

use super::sync::Explorer;
use super::tree::Tree;

/// Changes the tree of `hamlet`'s explorer by `change`.
fn change(
    world: &mut World,
    hamlet: Entity,
    change: impl FnOnce(&mut Tree),
) {
    if let Some(mut explorer) = world.get_mut::<Explorer>(hamlet) {
        change(&mut explorer.0);
    }
}

fn root(world: &World, hamlet: Entity) -> Option<PathBuf> {
    world
        .get::<Explorer>(hamlet)
        .map(|explorer| explorer.0.root().to_path_buf())
}

/// Selects `entry`, and opens or closes it if it is a folder or opens
/// it in the system's program for it if it is a file.
pub(super) fn press(
    world: &mut World,
    hamlet: Entity,
    entry: &Entry,
) {
    change(world, hamlet, |tree| {
        tree.select(&entry.path);
        if entry.is_folder() {
            tree.toggle(&entry.path);
        }
    });
    if !entry.is_folder() {
        show(world, hamlet, &entry.path);
    }
}

/// Closes every folder of `hamlet`'s explorer.
pub(super) fn collapse(world: &mut World, hamlet: Entity) {
    change(world, hamlet, Tree::collapse);
}

/// Opens the file `path` in the system's program for it.
pub(super) fn show(world: &mut World, hamlet: Entity, path: &Path) {
    let shown = root(world, hamlet)
        .is_some_and(|root| open::show(path, &[root]));
    if !shown {
        bevy::log::warn!("could not open {}", path.display());
    }
}

/// Shows `path` in its folder in the system's file manager.
pub(super) fn reveal(world: &mut World, hamlet: Entity, path: &Path) {
    let shown = root(world, hamlet)
        .is_some_and(|root| open::reveal(path, &[root]));
    if !shown {
        bevy::log::warn!("could not reveal {}", path.display());
    }
}

/// Copies `path` whole.
pub(super) fn copy_path(world: &mut World, path: &Path) {
    let whole = path.display().to_string();
    // The long form Windows gives a canonical path, which few
    // programs read.
    let whole = whole.strip_prefix(r"\\?\").unwrap_or(&whole);
    copy(world, whole.to_string());
}

/// Copies `path` from the root of `hamlet`'s explorer.
pub(super) fn copy_relative(
    world: &mut World,
    hamlet: Entity,
    path: &Path,
) {
    let Some(root) = root(world, hamlet) else {
        return;
    };
    let relative = path.strip_prefix(&root).unwrap_or(path);
    copy(world, relative.display().to_string());
}

fn copy(world: &mut World, text: String) {
    let Some(mut clipboard) = world.get_resource_mut::<Clipboard>()
    else {
        return;
    };
    if let Err(e) = clipboard.set_text(text) {
        bevy::log::warn!("could not copy the path: {e:?}");
    }
}
