//! What a plugin declares when it is enabled.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy_fynix::shortcut::Chord;

use super::{ContributedBy, Status};
use crate::AnyView;
use crate::command::{Command, Handler, HandlerWith, Opens, Subject};
use crate::context::{ContextItem, ContextWhen};
use crate::dock::Panel;
use crate::icons::Glyph;
use crate::inspector::InspectorSection;
use crate::keys::Key;
use crate::menu::{Menu, MenuEntry};
use crate::page::{Native, ShownWhen};
use crate::settings::SettingsPage;

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
    Menu(MenuEntry),
    Key(Key),
    Context(ContextItem, Option<fn(&World, &Subject) -> bool>),
    Inspector(InspectorSection, Option<NativePage>),
    Settings(SettingsPage, Option<fn() -> AnyView>),
}

/// A page a built-in plugin draws itself: its view, and when it is
/// there.
type NativePage = (fn() -> AnyView, fn(&World) -> bool);

/// What a command does when it is run.
enum Does {
    Handler(fn(&mut Commands)),
    HandlerWith(fn(&mut Commands, &Subject)),
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

    /// Adds a command that `handler` carries out on the [`Subject`]
    /// it is run on.
    pub fn command_with(
        &mut self,
        id: &str,
        title: &str,
        handler: fn(&mut Commands, &Subject),
    ) {
        self.declare(id, title, None, Does::HandlerWith(handler));
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

    /// Adds a line to `menu` that runs the plugin's command
    /// `command`. Lines are grouped by `section`, in order, with a
    /// line between groups.
    pub fn menu_item(
        &mut self,
        menu: Menu,
        section: u32,
        label: &str,
        command: &str,
        icon: Option<Glyph>,
    ) {
        self.declared.push(Declared::Menu(MenuEntry {
            menu,
            section,
            label: label.into(),
            command: command.into(),
            icon,
        }));
    }

    /// Adds a shortcut that runs the plugin's command `command`.
    pub fn key(&mut self, chord: Chord, command: &str) {
        self.declared.push(Declared::Key(Key {
            chord,
            command: command.into(),
        }));
    }

    /// Adds an item to the menu of the things of kind `target` that
    /// runs the plugin's command `command` on the thing. `label`
    /// may hold `{name}`, the thing's name.
    pub fn context_item(
        &mut self,
        target: &str,
        section: u32,
        label: &str,
        command: &str,
        icon: Option<Glyph>,
    ) {
        self.declare_context(
            target, section, label, command, icon, None,
        );
    }

    /// Adds an item as `context_item` does, shown only for the things
    /// `when` holds for.
    pub fn context_item_if(
        &mut self,
        target: &str,
        section: u32,
        label: &str,
        command: &str,
        when: fn(&World, &Subject) -> bool,
    ) {
        self.declare_context(
            target,
            section,
            label,
            command,
            None,
            Some(when),
        );
    }

    /// Adds a tab to the inspector's page of the things of kind
    /// `target`, whose page is the plugin panel `panel`, by its id in
    /// the dock.
    pub fn inspector_section(
        &mut self,
        target: &str,
        title: &str,
        panel: &str,
    ) {
        self.declare_section(target, title, panel, None);
    }

    /// Adds a tab as `inspector_section` does, whose page `view`
    /// draws, there only while `when` holds. The tab takes its icon
    /// from the panel `panel`.
    pub fn inspector_section_with(
        &mut self,
        target: &str,
        title: &str,
        panel: &str,
        view: fn() -> AnyView,
        when: fn(&World) -> bool,
    ) {
        self.declare_section(
            target,
            title,
            panel,
            Some((view, when)),
        );
    }

    /// Adds a door to the Settings panel, to a page that is the
    /// plugin panel `panel`, by its id in the dock.
    pub fn settings_page(
        &mut self,
        title: &str,
        icon: Glyph,
        panel: &str,
    ) {
        self.declared.push(Declared::Settings(
            SettingsPage {
                title: title.into(),
                icon,
                holds: String::new(),
                panel: panel.into(),
            },
            None,
        ));
    }

    /// Adds a door to the Settings panel, to a page that `view`
    /// draws, with `holds` under its title.
    pub fn settings_page_with(
        &mut self,
        title: &str,
        icon: Glyph,
        holds: &str,
        view: fn() -> AnyView,
    ) {
        self.declared.push(Declared::Settings(
            SettingsPage {
                title: title.into(),
                icon,
                holds: holds.into(),
                panel: String::new(),
            },
            Some(view),
        ));
    }

    fn declare_section(
        &mut self,
        target: &str,
        title: &str,
        panel: &str,
        page: Option<NativePage>,
    ) {
        self.declared.push(Declared::Inspector(
            InspectorSection {
                target: target.into(),
                title: title.into(),
                panel: panel.into(),
            },
            page,
        ));
    }

    fn declare_context(
        &mut self,
        target: &str,
        section: u32,
        label: &str,
        command: &str,
        icon: Option<Glyph>,
        when: Option<fn(&World, &Subject) -> bool>,
    ) {
        self.declared.push(Declared::Context(
            ContextItem {
                target: target.into(),
                section,
                label: label.into(),
                command: command.into(),
                icon,
            },
            when,
        ));
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
                Declared::Menu(entry) => {
                    commands.spawn((entry, ContributedBy(plugin)));
                }
                Declared::Key(key) => {
                    commands.spawn((key, ContributedBy(plugin)));
                }
                Declared::Inspector(section, page) => {
                    let mut section = commands
                        .spawn((section, ContributedBy(plugin)));
                    if let Some((view, when)) = page {
                        section
                            .insert((Native(view), ShownWhen(when)));
                    }
                }
                Declared::Settings(page, view) => {
                    let mut page =
                        commands.spawn((page, ContributedBy(plugin)));
                    if let Some(view) = view {
                        page.insert(Native(view));
                    }
                }
                Declared::Context(item, when) => {
                    let mut item =
                        commands.spawn((item, ContributedBy(plugin)));
                    if let Some(when) = when {
                        item.insert(ContextWhen(when));
                    }
                }
                Declared::Command(command, does) => {
                    let mut command = commands
                        .spawn((command, ContributedBy(plugin)));
                    match does {
                        Does::Handler(handler) => {
                            command.insert(Handler(handler));
                        }
                        Does::HandlerWith(handler) => {
                            command.insert(HandlerWith(handler));
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
