//! What a plugin declares when it is enabled.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;

use super::{ContributedBy, Status};
use crate::AnyView;
use crate::dock::Panel;
use crate::icons::Glyph;

/// The declarations of one plugin. It reaches neither the app nor the
/// world.
#[derive(Default)]
pub struct Entry {
    declared: Vec<Declared>,
}

/// One thing a plugin adds. A new kind of contribution is a variant
/// here, a method of [`Entry`] and an arm of [`Entry::spawn`].
enum Declared {
    Panel(Panel),
}

impl Entry {
    /// Adds a panel to the dock.
    pub fn panel(
        &mut self,
        id: &str,
        title: &str,
        icon: Glyph,
        view: fn() -> AnyView,
    ) {
        self.declared
            .push(Declared::Panel(Panel::new(id, title, icon, view)));
    }

    /// Adds a panel to the dock, built from its id.
    pub fn panel_by_id(
        &mut self,
        id: &str,
        title: &str,
        icon: Glyph,
        view: fn(&str) -> AnyView,
    ) {
        self.declared.push(Declared::Panel(Panel::by_id(
            id, title, icon, view,
        )));
    }

    /// Spawns what was declared, each as an entity of `plugin`.
    pub fn spawn(self, plugin: Entity, commands: &mut Commands) {
        for declared in self.declared {
            match declared {
                Declared::Panel(panel) => {
                    commands.spawn((panel, ContributedBy(plugin)));
                }
            }
        }
    }
}

/// On the entity of a plugin: its `entry`, run each time it is
/// enabled.
#[derive(Component, Clone, Copy)]
#[component(immutable)]
pub struct EntryPoint(pub fn(&mut Entry));

impl EntryPoint {
    /// Runs the entry for `plugin` and marks it running.
    pub(super) fn run(self, plugin: Entity, commands: &mut Commands) {
        let mut entry = Entry::default();
        (self.0)(&mut entry);
        entry.spawn(plugin, commands);
        commands.entity(plugin).insert(Status::Running);
    }
}
