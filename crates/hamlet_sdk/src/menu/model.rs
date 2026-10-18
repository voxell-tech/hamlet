//! The lines the menus show, for the bar that draws them.

use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Added, Changed, Or};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};

use super::{Menu, MenuEntry};
use crate::capability::{Capability, Granted, allowed};
use crate::command::{Command, full};
use crate::icons::Glyph;
use crate::plugin::{ContributedBy, PluginInfo};

/// Every line that may show, in the order a menu lists them: by
/// section, then label, then plugin.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Menus(pub Vec<Line>);

/// One line of a menu.
#[derive(Clone, PartialEq, Debug)]
pub struct Line {
    pub menu: Menu,
    pub section: u32,
    pub label: String,
    pub icon: Option<Glyph>,
    /// The id [`RunCommand`](crate::command::RunCommand) takes.
    pub command: String,
    /// The id of the plugin that added it.
    pub plugin: String,
}

impl Menus {
    /// The lines of `menu`, a group for each section.
    pub fn sections(&self, menu: Menu) -> Vec<Vec<&Line>> {
        let mut sections = Vec::<Vec<&Line>>::new();
        for line in self.0.iter().filter(|line| line.menu == menu) {
            match sections.last_mut() {
                Some(last) if last[0].section == line.section => {
                    last.push(line)
                }
                _ => sections.push(vec![line]),
            }
        }
        sections
    }
}

pub(super) fn moved(
    changed: Query<
        (),
        Or<(
            Added<MenuEntry>,
            Added<Command>,
            Changed<Granted>,
            Changed<PluginInfo>,
        )>,
    >,
    mut entries: RemovedComponents<MenuEntry>,
    mut commands: RemovedComponents<Command>,
) -> bool {
    !changed.is_empty()
        || entries.read().count() > 0
        || commands.read().count() > 0
}

pub(super) fn keep(
    entries: Query<(&MenuEntry, &ContributedBy)>,
    commands: Query<(&Command, &ContributedBy)>,
    plugins: Query<(&PluginInfo, Option<&Granted>)>,
    mut menus: ResMut<Menus>,
) {
    let mut now = entries
        .iter()
        .filter_map(|(entry, by)| {
            let (info, granted) = plugins.get(by.0).ok()?;
            if !allowed(info, granted, Capability::UiMenu) {
                return None;
            }
            let (command, _) =
                commands.iter().find(|(command, of)| {
                    of.0 == by.0 && command.id == entry.command
                })?;
            Some(Line {
                menu: entry.menu,
                section: entry.section,
                label: entry.label.clone(),
                icon: entry.icon.or(command.icon),
                command: full(&info.id, &command.id),
                plugin: info.id.clone(),
            })
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| {
        (a.section, &a.label, &a.plugin)
            .cmp(&(b.section, &b.label, &b.plugin))
    });
    if menus.0 != now {
        menus.0 = now;
    }
}
