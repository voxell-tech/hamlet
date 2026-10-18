//! Running a command by its id.

use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use bevy::log::warn;

use super::{Command, Handler, HandlerWith, Notice, Opens, Subject};
use crate::dock;
use crate::plugin::{ContributedBy, PluginInfo, Status};

/// Run the command of this id: the plugin that owns it, a slash and
/// the name it exports it by. The chief's own, as a click on a menu
/// is.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct RunCommand {
    pub id: String,
    /// The thing it is run on, where it is run on one.
    pub subject: Option<Subject>,
}

impl RunCommand {
    /// Runs the command `id` on nothing.
    pub fn of(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            subject: None,
        }
    }

    /// Runs the command `id` on `subject`.
    pub fn on(id: impl Into<String>, subject: Subject) -> Self {
        Self {
            id: id.into(),
            subject: Some(subject),
        }
    }
}

/// A command that the plugin which owns it carries out in code of its
/// own, for whoever hosts that code to pass on.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct Forward {
    pub plugin: Entity,
    /// The name the plugin exports the command by.
    pub command: String,
    /// The [`Subject`] it was run on, as [`Subject::encode`] writes
    /// it, or nothing.
    pub args: Vec<u8>,
}

pub(super) fn run(
    run: On<RunCommand>,
    plugins: Query<(Entity, &PluginInfo, &Status)>,
    commands: Query<(
        &Command,
        &ContributedBy,
        Option<&Handler>,
        Option<&HandlerWith>,
        Option<&Opens>,
    )>,
    mut out: Commands,
) {
    let id = &run.id;
    let Some((owner, name)) = id.split_once('/') else {
        return refuse(&mut out, format!("`{id}` is not a command"));
    };
    let Some((plugin, info, status)) =
        plugins.iter().find(|(_, info, _)| info.id == owner)
    else {
        return refuse(&mut out, format!("no plugin `{owner}`"));
    };
    if *status != Status::Running {
        return refuse(
            &mut out,
            format!(
                "`{id}` did not run: {} is {}",
                info.name,
                status.words()
            ),
        );
    }
    let Some((.., handler, with, opens)) =
        commands.iter().find(|(command, by, ..)| {
            by.0 == plugin && command.id == name
        })
    else {
        return refuse(&mut out, format!("{owner} has no `{name}`"));
    };
    match (handler, with, opens, &run.subject) {
        (Some(Handler(handler)), ..) => handler(&mut out),
        (_, Some(HandlerWith(handler)), _, Some(subject)) => {
            handler(&mut out, subject)
        }
        (_, Some(_), ..) => refuse(
            &mut out,
            format!("`{id}` needs something to act on"),
        ),
        (.., Some(Opens(panel)), _) => {
            let panel = panel.clone();
            out.queue(move |world: &mut World| {
                dock::open(world, &panel)
            });
        }
        (.., subject) => out.trigger(Forward {
            plugin,
            command: name.into(),
            args: subject
                .as_ref()
                .map(Subject::encode)
                .unwrap_or_default(),
        }),
    }
}

fn refuse(out: &mut Commands, why: String) {
    warn!("{why}");
    out.trigger(Notice(why));
}
