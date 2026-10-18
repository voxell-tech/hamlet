//! The items the menus of the world show, for the menu that draws
//! them.

use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Added, Changed, Or};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};

use super::ContextItem;
use crate::capability::{Capability, Granted, allowed};
use crate::command::{Command, Subject, full};
use crate::icons::Glyph;
use crate::plugin::{ContributedBy, PluginInfo};

/// Every item that may show, in the order a menu lists them: by
/// section, then label, then plugin.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct ContextItems(pub Vec<Line>);

/// One item of a menu.
#[derive(Clone, PartialEq, Debug)]
pub struct Line {
    /// The entity of its [`ContextItem`].
    pub item: Entity,
    pub target: String,
    pub section: u32,
    pub label: String,
    pub icon: Option<Glyph>,
    /// The id [`RunCommand`](crate::command::RunCommand) takes.
    pub command: String,
    /// The id of the plugin that added it.
    pub plugin: String,
}

impl Line {
    /// What the item says on `subject`.
    pub fn said(&self, subject: &Subject) -> String {
        self.label.replace("{name}", &subject.name)
    }
}

impl ContextItems {
    /// The items in the menu of the things of this kind.
    pub fn of<'a>(
        &'a self,
        kind: &'a str,
    ) -> impl Iterator<Item = &'a Line> {
        self.0.iter().filter(move |line| line.target == kind)
    }
}

pub(super) fn moved(
    changed: Query<
        (),
        Or<(
            Added<ContextItem>,
            Added<Command>,
            Changed<Granted>,
            Changed<PluginInfo>,
        )>,
    >,
    mut items: RemovedComponents<ContextItem>,
    mut commands: RemovedComponents<Command>,
) -> bool {
    !changed.is_empty()
        || items.read().count() > 0
        || commands.read().count() > 0
}

pub(super) fn keep(
    items: Query<(Entity, &ContextItem, &ContributedBy)>,
    commands: Query<(&Command, &ContributedBy)>,
    plugins: Query<(&PluginInfo, Option<&Granted>)>,
    mut shown: ResMut<ContextItems>,
) {
    let mut now = items
        .iter()
        .filter_map(|(item, context, by)| {
            let (info, granted) = plugins.get(by.0).ok()?;
            if !allowed(info, granted, Capability::UiContext) {
                return None;
            }
            let (command, _) =
                commands.iter().find(|(command, of)| {
                    of.0 == by.0 && command.id == context.command
                })?;
            Some(Line {
                item,
                target: context.target.clone(),
                section: context.section,
                label: context.label.clone(),
                icon: context.icon.or(command.icon),
                command: full(&info.id, &command.id),
                plugin: info.id.clone(),
            })
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| {
        (a.section, &a.label, &a.plugin)
            .cmp(&(b.section, &b.label, &b.plugin))
    });
    if shown.0 != now {
        shown.0 = now;
    }
}
