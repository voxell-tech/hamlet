//! The files the person dropped on a plugin's views, each handed to
//! the plugin by a number. A service given such a number takes the
//! file the person dropped, never a path the plugin names.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;

/// The files dropped on each plugin's views, by their numbers.
#[derive(Resource, Default, Debug)]
pub struct DroppedFiles {
    next: u32,
    given: HashMap<Entity, Vec<(u32, PathBuf)>>,
}

impl DroppedFiles {
    /// Hands `path` to `plugin`: the number it knows it by.
    pub fn give(&mut self, plugin: Entity, path: PathBuf) -> u32 {
        self.next = self.next.wrapping_add(1).max(1);
        self.given
            .entry(plugin)
            .or_default()
            .push((self.next, path));
        self.next
    }

    /// The file dropped on `plugin`'s views with the number `id`.
    pub fn get(&self, plugin: Entity, id: u32) -> Option<&Path> {
        self.given
            .get(&plugin)?
            .iter()
            .find(|(given, _)| *given == id)
            .map(|(_, path)| path.as_path())
    }

    /// Forgets every file handed to `plugin`.
    pub fn forget(&mut self, plugin: Entity) {
        self.given.remove(&plugin);
    }
}
