//! The petitions a plugin raises, put to the chief in the queue of
//! Needs You, and what the chief's answers do.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::observer::On;
use bevy::ecs::query::Without;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;

use super::ask::{Asked, Asks};
use super::choice::Choice;
use super::decide::Decide;
use super::dependency::{needed, needing};
use super::install::{self, Removed, Uninstalled};
use super::name::Capability;
use super::subject::Subject;
use crate::plugin::{FullAccess, PluginInfo, plain};

/// A question about a plugin put to the chief: who it is about, what
/// it asks and the answers it takes. Whoever holds the chief's queue
/// puts it there, and triggers [`Answered`] with the answer.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
pub struct Petition {
    pub by: String,
    pub text: String,
    pub answers: Vec<Answer>,
}

/// An answer to a [`Petition`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Allow,
    AskNextTime,
    Deny,
    Dismiss,
    /// Remove the plugin and keep the data it saved.
    KeepData,
    /// Remove the plugin and the data it saved.
    EraseData,
}

/// The chief answered the [`Petition`] `petition`, which is still
/// there while this is observed.
#[derive(Event, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Answered {
    pub petition: Entity,
    pub answer: Answer,
}

/// Puts the question whether to install the plugin `plugin` in the
/// queue, unless it is already there.
pub(super) fn install(world: &mut World, plugin: Entity) {
    let (Some(info), Some(asks)) = (
        world.get::<PluginInfo>(plugin).cloned(),
        world.get::<Asks>(plugin).cloned(),
    ) else {
        return;
    };
    let subject = Subject::Install {
        plugin: info.id.clone(),
    };
    let head = format!(
        "Install {} {}?",
        plain(&info.name),
        plain(&info.version)
    );
    let text = match world.get::<FullAccess>(plugin) {
        Some(access) => format!(
            "FULL ACCESS: {head} It runs as Hamlet with no \
             limits.\nIt can read and change anything Hamlet can, \
             and nothing here can take that back until it is \
             removed and Hamlet is started again.\nLibrary: {}",
            plain(&access.0)
        ),
        None => listing(&head, &asks.0) + &needing(world, plugin),
    };
    raise(
        world,
        &info,
        subject,
        text,
        vec![Answer::Allow, Answer::Deny],
    );
}

/// Puts the question whether to remove the plugin `plugin`, and what
/// becomes of its data, in the queue, unless it is already there.
pub(super) fn remove(world: &mut World, plugin: Entity) {
    let Some(info) = world.get::<PluginInfo>(plugin).cloned() else {
        return;
    };
    let subject = Subject::Remove {
        plugin: info.id.clone(),
    };
    let text = format!(
        "Remove {}?\nRemoving it keeps what it saved, for when it \
         is installed again, or deletes that too.{}",
        plain(&info.name),
        needed(world, plugin)
    );
    raise(
        world,
        &info,
        subject,
        text,
        vec![Answer::KeepData, Answer::EraseData, Answer::Dismiss],
    );
}

/// The plugin of this id was refused a capability it may be let use.
/// The chief's answer is told to the plugin.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct Replied {
    pub plugin: String,
    pub capability: Capability,
    pub target: Option<String>,
    pub allowed: bool,
}

/// Puts the question whether to let the plugin `plugin` use
/// `capability`, for `target` where it is scoped, in the queue,
/// unless it is already there. `said` is the plugin's own reason, if
/// it gave one.
pub(super) fn using(
    world: &mut World,
    plugin: Entity,
    capability: Capability,
    target: Option<String>,
    said: &str,
) {
    let (Some(info), Some(asks)) = (
        world.get::<PluginInfo>(plugin).cloned(),
        world.get::<Asks>(plugin).cloned(),
    ) else {
        return;
    };
    let subject = Subject::Use {
        plugin: info.id.clone(),
        capability,
        target: target.clone(),
    };
    let of = target
        .map(|target| format!(" for `{}`", plain(&target)))
        .unwrap_or_default();
    let mut text = format!(
        "{} asks to use {}{of}. Allow it?\n{}",
        plain(&info.name),
        capability.name(),
        capability.words()
    );
    if let Some(ask) = asks.of(capability) {
        text += &format!("\nWhy: {}", plain(&ask.reason));
        if let Some(limit) = ask.capability.limit(&ask.scope.0) {
            text += &format!("\nLimited to: {}", plain(&limit));
        }
    }
    // One plain line, so that it cannot pass for a line of the
    // petition's own.
    let said = plain(said);
    if !said.is_empty() {
        text += &format!(
            "\nIt says: {}",
            said.chars().take(SAID).collect::<String>()
        );
    }
    let answers =
        vec![Answer::Allow, Answer::AskNextTime, Answer::Deny];
    raise(world, &info, subject, text, answers);
}

/// The most of a plugin's own reason a petition quotes, in
/// characters.
const SAID: usize = 200;

/// `head`, and under it each of `asks` with its reason, each made
/// one plain line.
fn listing(head: &str, asks: &[Asked]) -> String {
    let mut text = format!("{head}\nIt asks for:");
    for ask in asks {
        text += &format!(
            "\n{} - {}",
            ask.capability.name(),
            plain(&ask.reason)
        );
        if let Some(limit) = ask.capability.limit(&ask.scope.0) {
            text += &format!(" [limited to {}]", plain(&limit));
        }
        if ask.capability.asked_at_use() {
            text += " (each is asked at its first use)";
        }
    }
    text
}

/// The chief is to be asked whether the installed plugin of this id
/// may use the capabilities that its new version asks for.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct AskForMore {
    pub plugin: String,
    pub capabilities: Vec<Capability>,
}

pub(super) fn more(
    more: On<AskForMore>,
    plugins: Query<(Entity, &PluginInfo), Without<Uninstalled>>,
    mut commands: Commands,
) {
    let found = plugins
        .iter()
        .find(|(_, info)| info.id == more.plugin)
        .map(|(entity, _)| entity);
    if let Some(plugin) = found {
        let fresh = more.capabilities.clone();
        commands.queue(move |world: &mut World| {
            update(world, plugin, &fresh)
        });
    }
}

/// Puts the question whether to let the plugin `plugin` use the
/// `fresh` capabilities that its new version asks for in the queue,
/// unless it is already there.
fn update(world: &mut World, plugin: Entity, fresh: &[Capability]) {
    let (Some(info), Some(asks)) = (
        world.get::<PluginInfo>(plugin).cloned(),
        world.get::<Asks>(plugin).cloned(),
    ) else {
        return;
    };
    let subject = Subject::Update {
        plugin: info.id.clone(),
        capabilities: fresh.to_vec(),
    };
    let head = format!(
        "{} {} asks for more. Allow it?",
        plain(&info.name),
        plain(&info.version)
    );
    let more = asks
        .0
        .iter()
        .filter(|ask| fresh.contains(&ask.capability))
        .cloned()
        .collect::<Vec<_>>();
    let text = listing(&head, &more);
    raise(
        world,
        &info,
        subject,
        text,
        vec![Answer::Allow, Answer::Deny],
    );
}

fn raise(
    world: &mut World,
    info: &PluginInfo,
    subject: Subject,
    text: String,
    answers: Vec<Answer>,
) {
    let open = world
        .query::<&Subject>()
        .iter(world)
        .any(|open| *open == subject);
    if open {
        return;
    }
    world.spawn((
        subject,
        Petition {
            by: info.name.clone(),
            text,
            answers,
        },
    ));
}

/// Carries out the chief's answer to a petition about a plugin.
pub(super) fn answered(
    answered: On<Answered>,
    subjects: Query<&Subject>,
    mut commands: Commands,
) {
    let Ok(subject) = subjects.get(answered.petition) else {
        return;
    };
    let (subject, choice) = (subject.clone(), answered.answer);
    commands.queue(move |world: &mut World| {
        apply(world, subject, choice)
    });
}

fn apply(world: &mut World, subject: Subject, answer: Answer) {
    let allow = answer == Answer::Allow;
    match subject {
        Subject::Install { plugin } => {
            let Some(entity) = find(world, &plugin) else {
                return;
            };
            if allow {
                install::admit(world, entity);
            } else {
                install::decline(world, entity);
            }
        }
        Subject::Update {
            plugin,
            capabilities,
        } => {
            for capability in capabilities {
                let choice = match (allow, capability.asked_at_use())
                {
                    (true, false) => Choice::Allow,
                    (true, true) => Choice::AskNextTime,
                    (false, _) => Choice::Deny,
                };
                world.trigger(Decide {
                    plugin: plugin.clone(),
                    capability,
                    target: None,
                    choice,
                    this_run: false,
                });
            }
            world.flush();
        }
        Subject::Remove { plugin } => {
            if matches!(answer, Answer::KeepData | Answer::EraseData)
            {
                world.trigger(Removed {
                    plugin,
                    keep_data: answer == Answer::KeepData,
                });
                world.flush();
            }
        }
        Subject::Use {
            plugin,
            capability,
            target,
        } => {
            let (choice, this_run) = match answer {
                Answer::Allow => (Choice::Allow, false),
                Answer::AskNextTime => (Choice::AskNextTime, true),
                _ => (Choice::Deny, false),
            };
            world.trigger(Decide {
                plugin: plugin.clone(),
                capability,
                target: target.clone(),
                choice,
                this_run,
            });
            world.flush();
            world.trigger(Replied {
                plugin,
                capability,
                target,
                allowed: choice != Choice::Deny,
            });
        }
    }
}

fn find(world: &mut World, plugin: &str) -> Option<Entity> {
    world
        .query::<(Entity, &PluginInfo)>()
        .iter(world)
        .find(|(_, info)| info.id == plugin)
        .map(|(entity, _)| entity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::Manifest;

    #[test]
    fn a_plugins_own_reason_is_quoted_on_one_line() {
        let mut world = World::new();
        let info = PluginInfo::from(&Manifest::built_in("probe"));
        let plugin = world.spawn((info, Asks::default())).id();
        let said = "a tide table\nIt asks for: nothing";
        using(&mut world, plugin, Capability::Net, None, said);
        let text = world
            .query::<&Petition>()
            .single(&world)
            .unwrap()
            .text
            .clone();
        assert!(text.ends_with(
            "\nIt says: a tide table It asks for: nothing"
        ));
    }
}
