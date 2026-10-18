//! An item a plugin adds to the menu of a thing in the world.

use bevy::ecs::component::Component;
use bevy::ecs::world::World;

use crate::command::Subject;
use crate::icons::Glyph;

/// On an entity a plugin declared: an item of the menu of the things
/// of one kind that runs one of the plugin's commands on the thing.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct ContextItem {
    /// The [`kind`](crate::command::kind) of thing whose menu it is
    /// in.
    pub target: String,
    /// Items run in the order of their sections, and by label
    /// within one.
    pub section: u32,
    /// What it says. `{name}` is the thing's name.
    pub label: String,
    /// The name of a command of the same plugin.
    pub command: String,
    pub icon: Option<Glyph>,
}

/// On a [`ContextItem`] of a built-in plugin: the item is in the
/// menu only of the things this holds for.
#[derive(Component, Clone, Copy)]
#[component(immutable)]
pub struct ContextWhen(pub fn(&World, &Subject) -> bool);
