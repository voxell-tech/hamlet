//! What a plugin declares when it is enabled.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;

use super::{ContributedBy, Status};
use crate::AnyView;
use crate::command::{Command, Handler, Opens};
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
    Command(Command, Does),
}

/// What a command does when it is run.
enum Does {
    Handler(fn(&mut Commands)),
    Opens(String),
    /// The plugin carries it out in code the app does not hold.
    Forwarded,
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

    /// Adds a command that `handler` carries out.
    pub fn command(
        &mut self,
        id: &str,
        title: &str,
        handler: fn(&mut Commands),
    ) {
        self.declare(id, title, None, Does::Handler(handler));
    }

    /// Adds a command that brings the panel `panel` forward.
    pub fn opens(&mut self, id: &str, title: &str, panel: &str) {
        self.declare(id, title, None, Does::Opens(panel.into()));
    }

    /// Adds a command that the plugin carries out itself, when it is
    /// told it was run.
    pub fn forwarded(
        &mut self,
        id: &str,
        title: &str,
        icon: Option<Glyph>,
    ) {
        self.declare(id, title, icon, Does::Forwarded);
    }

    fn declare(
        &mut self,
        id: &str,
        title: &str,
        icon: Option<Glyph>,
        does: Does,
    ) {
        let command = Command::new(id, title).icon(icon);
        self.declared.push(Declared::Command(command, does));
    }

    /// Spawns what was declared, each as an entity of `plugin`.
    pub fn spawn(self, plugin: Entity, commands: &mut Commands) {
        for declared in self.declared {
            match declared {
                Declared::Panel(panel) => {
                    commands.spawn((panel, ContributedBy(plugin)));
                }
                Declared::Command(command, does) => {
                    let mut command = commands
                        .spawn((command, ContributedBy(plugin)));
                    match does {
                        Does::Handler(handler) => {
                            command.insert(Handler(handler));
                        }
                        Does::Opens(panel) => {
                            command.insert(Opens(panel));
                        }
                        Does::Forwarded => {}
                    }
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
