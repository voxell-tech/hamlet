//! The presses in an explorer, and their effects.

use std::path::{Path, PathBuf};

use bevy::clipboard::Clipboard;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use hamlet_desktop::files::Entry;
use hamlet_desktop::open::{self, Launch};

use super::editor;
use super::sync::Explorer;
use super::tree::Tree;
use crate::sounds;

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

/// Selects `entry`, and opens or closes it if it is a folder or
/// opens it in the editor if it is a file.
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
        open(world, hamlet, &entry.path);
    }
}

/// Opens the file `path` in `hamlet`'s editor.
pub(super) fn open(world: &mut World, hamlet: Entity, path: &Path) {
    if let Some(root) = root(world, hamlet) {
        editor::open(world, hamlet, path, &root);
    }
}

/// Closes every folder of `hamlet`'s explorer.
pub(super) fn collapse(world: &mut World, hamlet: Entity) {
    change(world, hamlet, Tree::collapse);
}

/// Opens the file `path` in the system's program for it.
pub(super) fn open_outside(
    world: &mut World,
    hamlet: Entity,
    path: &Path,
) {
    let launch = match root(world, hamlet) {
        Some(root) => open::show(path, &[root]),
        None => Launch::Failed,
    };
    if launch == Launch::Failed {
        sounds::denied(world);
    }
}

/// Shows `path` in its folder in the system's file manager.
pub(super) fn reveal(world: &mut World, hamlet: Entity, path: &Path) {
    let launch = match root(world, hamlet) {
        Some(root) => open::reveal(path, &[root]),
        None => Launch::Failed,
    };
    if launch == Launch::Failed {
        sounds::denied(world);
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
