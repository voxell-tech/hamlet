//! The commands there are, for the views that list them.

use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Added, Changed, Or};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};

use super::{Command, full};
use crate::plugin::{ContributedBy, PluginInfo, Status};

/// Every command, by title.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Commands(pub Vec<Listed>);

/// One command, as a view shows it.
#[derive(Clone, PartialEq, Debug)]
pub struct Listed {
    /// The id [`RunCommand`](super::RunCommand) takes.
    pub id: String,
    pub title: String,
    /// The id of the plugin that owns it.
    pub owner: String,
    /// Whether its plugin is running.
    pub enabled: bool,
}

pub(super) fn moved(
    changed: Query<(), Or<(Added<Command>, Changed<Status>)>>,
    mut gone: RemovedComponents<Command>,
) -> bool {
    !changed.is_empty() || gone.read().count() > 0
}

pub(super) fn keep(
    commands: Query<(&Command, &ContributedBy)>,
    plugins: Query<(&PluginInfo, &Status)>,
    mut listed: ResMut<Commands>,
) {
    let mut now = commands
        .iter()
        .filter_map(|(command, by)| {
            let (info, status) = plugins.get(by.0).ok()?;
            Some(Listed {
                id: full(&info.id, &command.id),
                title: command.title.clone(),
                owner: info.id.clone(),
                enabled: *status == Status::Running,
            })
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| (&a.title, &a.id).cmp(&(&b.title, &b.id)));
    if listed.0 != now {
        listed.0 = now;
    }
}
