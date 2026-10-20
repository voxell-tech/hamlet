//! A plugin's one entry point, and what it declares there.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use hamlet_engine_api::{
    CommandId, IdError, Layout, PanelId, PluginId,
};
use hamlet_engine_core::Budgets;
use hamlet_engine_shell::command::parse_chord;
use hamlet_engine_shell::icons::Glyph;
use hamlet_engine_shell::{
    AnyView, Command, MenuEntry, Overlay, Panel, RunCommand,
    SettingsFooter, SettingsPage, Shortcut, StatusItem,
    SuggestedWorkspace,
};

use crate::switch::ContributedBy;

/// On a plugin's entity: its entry point, run each time it is
/// switched on.
#[derive(Component, Clone, Copy)]
#[component(immutable)]
pub struct EntryPoint(pub fn(&mut Entry));

/// What a plugin adds, as its entry point declares it. It reaches
/// neither the app nor the world: the engine adds what it holds,
/// linked to the plugin.
///
/// A name is the plugin's own, and its full id is the plugin's id and
/// the name. A command is named by its name, for one of the plugin's
/// own, or by its full id.
#[derive(Default)]
pub struct Entry {
    declared: Vec<Declared>,
}

enum Declared {
    Panel {
        name: String,
        title: String,
        content: fn(Entity) -> AnyView,
    },
    Command {
        name: String,
        label: String,
        run: fn(&mut World, Entity),
    },
    MenuEntry {
        menu: String,
        command: String,
    },
    Shortcut {
        keys: String,
        command: String,
    },
    SettingsPage {
        name: String,
        title: String,
        about: String,
        icon: Glyph,
        content: fn(Entity) -> AnyView,
    },
    SettingsFooter {
        name: String,
        content: fn(Entity) -> AnyView,
    },
    Workspace {
        name: String,
        title: String,
        layout: Layout,
    },
    StatusItem {
        name: String,
    },
    Overlay {
        name: String,
        content: fn(Entity) -> AnyView,
    },
}

impl Entry {
    /// A panel for the dock, titled `title`.
    pub fn panel(
        &mut self,
        name: &str,
        title: &str,
        content: fn(Entity) -> AnyView,
    ) -> &mut Self {
        self.declared.push(Declared::Panel {
            name: name.into(),
            title: title.into(),
            content,
        });
        self
    }

    /// A command a menu calls `label`, which runs `run` with the
    /// command's entity.
    pub fn command(
        &mut self,
        name: &str,
        label: &str,
        run: fn(&mut World, Entity),
    ) -> &mut Self {
        self.declared.push(Declared::Command {
            name: name.into(),
            label: label.into(),
            run,
        });
        self
    }

    /// The command `command` in the bar's menu titled `menu`.
    pub fn menu_entry(
        &mut self,
        menu: &str,
        command: &str,
    ) -> &mut Self {
        self.declared.push(Declared::MenuEntry {
            menu: menu.into(),
            command: command.into(),
        });
        self
    }

    /// The keys `keys` bound to the command `command`, written as
    /// `Primary+Shift+H`.
    pub fn shortcut(
        &mut self,
        keys: &str,
        command: &str,
    ) -> &mut Self {
        self.declared.push(Declared::Shortcut {
            keys: keys.into(),
            command: command.into(),
        });
        self
    }

    /// A page of settings titled `title`, its link row showing `icon`
    /// and what it holds, `about`.
    pub fn settings_page(
        &mut self,
        name: &str,
        title: &str,
        about: &str,
        icon: Glyph,
        content: fn(Entity) -> AnyView,
    ) -> &mut Self {
        self.declared.push(Declared::SettingsPage {
            name: name.into(),
            title: title.into(),
            about: about.into(),
            icon,
            content,
        });
        self
    }

    /// What Settings shows under the link rows to its pages.
    pub fn settings_footer(
        &mut self,
        name: &str,
        content: fn(Entity) -> AnyView,
    ) -> &mut Self {
        self.declared.push(Declared::SettingsFooter {
            name: name.into(),
            content,
        });
        self
    }

    /// A workspace the person can take, titled `title`.
    pub fn workspace(
        &mut self,
        name: &str,
        title: &str,
        layout: Layout,
    ) -> &mut Self {
        self.declared.push(Declared::Workspace {
            name: name.into(),
            title: title.into(),
            layout,
        });
        self
    }

    /// An item of the status line, hidden until it is given words.
    pub fn status_item(&mut self, name: &str) -> &mut Self {
        self.declared
            .push(Declared::StatusItem { name: name.into() });
        self
    }

    /// A card over the dock, shown while it is
    /// [`OverlayShown`](hamlet_engine_shell::OverlayShown).
    pub fn overlay(
        &mut self,
        name: &str,
        content: fn(Entity) -> AnyView,
    ) -> &mut Self {
        self.declared.push(Declared::Overlay {
            name: name.into(),
            content,
        });
        self
    }

    /// Adds what was declared, linked to `plugin`, whose id is
    /// `owner`, within `budgets`. Why each thing not added was not.
    pub(crate) fn spawn(
        self,
        plugin: Entity,
        owner: &PluginId,
        budgets: &Budgets,
        commands: &mut Commands,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        let (mut added, mut panels) = (0, 0);
        let link = ContributedBy(plugin);
        let owner = owner.as_str();
        for declared in self.declared {
            if added == budgets.contributions {
                problems.push(format!(
                    "adds more than {} things; the rest are left out",
                    budgets.contributions
                ));
                break;
            }
            let done = match declared {
                Declared::Panel {
                    name,
                    title,
                    content,
                } => {
                    if panels == budgets.panels {
                        problems.push(format!(
                            "{name}: more than {} panels",
                            budgets.panels
                        ));
                        continue;
                    }
                    panels += 1;
                    PanelId::new(owner, &name).map(|id| {
                        commands.spawn((Panel { id, title, content }, link));
                    })
                }
                Declared::Command { name, label, run } => {
                    CommandId::new(owner, &name).map(|id| {
                        commands.spawn((Command { id, label }, link)).observe(
                            move |event: On<RunCommand>, mut later: Commands| {
                                let command = event.entity;
                                later.queue(move |world: &mut World| {
                                    run(world, command);
                                });
                            },
                        );
                    })
                }
                Declared::MenuEntry { menu, command } => {
                    command_id(owner, &command).map(|command| {
                        commands.spawn((MenuEntry { menu, command }, link));
                    })
                }
                Declared::Shortcut { keys, command } => {
                    let Some(chord) = parse_chord(&keys) else {
                        problems.push(format!("{keys:?} are not keys"));
                        continue;
                    };
                    command_id(owner, &command).map(|command| {
                        commands.spawn((Shortcut { chord, command }, link));
                    })
                }
                Declared::SettingsPage {
                    name,
                    title,
                    about,
                    icon,
                    content,
                } => PanelId::new(owner, &name).map(|id| {
                    commands.spawn((
                        SettingsPage {
                            id,
                            title,
                            about,
                            icon,
                            content,
                        },
                        link,
                    ));
                }),
                Declared::SettingsFooter { name, content } => {
                    PanelId::new(owner, &name).map(|id| {
                        commands.spawn((SettingsFooter { id, content }, link));
                    })
                }
                Declared::Workspace {
                    name,
                    title,
                    layout,
                } => CommandId::new(owner, &format!("workspace.{name}")).map(
                    |take| {
                        commands.spawn((
                            SuggestedWorkspace {
                                take,
                                name: title,
                                layout,
                            },
                            link,
                        ));
                    },
                ),
                Declared::StatusItem { name } => {
                    PanelId::new(owner, &name).map(|id| {
                        commands.spawn((StatusItem::new(id), link));
                    })
                }
                Declared::Overlay { name, content } => {
                    PanelId::new(owner, &name).map(|id| {
                        commands.spawn((Overlay { id, content }, link));
                    })
                }
            };
            match done {
                Ok(()) => added += 1,
                Err(why) => problems.push(why.to_string()),
            }
        }
        problems
    }
}

/// The full id of the command `name`: one of `owner`'s by its name,
/// or any by its full id.
fn command_id(owner: &str, name: &str) -> Result<CommandId, IdError> {
    if name.contains('/') {
        CommandId::try_from(name)
    } else {
        CommandId::new(owner, name)
    }
}
