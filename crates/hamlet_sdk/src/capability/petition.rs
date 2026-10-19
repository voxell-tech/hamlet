//! The petitions a plugin raises, put to the chief in the queue of
//! Needs You, and what the chief's answers do.

use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::observer::On;
use bevy::ecs::query::Without;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use hamlet_core::oversight::{self, Settled};
use hamlet_rules::chief::rulings::Choice as Answer;

use super::ask::{Asked, Asks};
use super::choice::Choice;
use super::decide::Decide;
use super::dependency::{needed, needing};
use super::install::{self, Removed, Uninstalled};
use super::name::Capability;
use super::subject::Subject;
use crate::plugin::{FullAccess, PluginInfo};

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
    let head = format!("Install {} {}?", info.name, info.version);
    let text = match world.get::<FullAccess>(plugin) {
        Some(access) => format!(
            "{head}\nFULL ACCESS: this plugin runs as Hamlet with \
             no limits. It can read and change anything Hamlet can, \
             and nothing here can take that back until it is \
             removed and Hamlet is started again.\nLibrary: {}",
            access.0
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
        info.name,
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
        .map(|target| format!(" for `{target}`"))
        .unwrap_or_default();
    let mut text = format!(
        "{} asks to use {}{of}. Allow it?\n{}",
        info.name,
        capability.name(),
        capability.words()
    );
    if let Some(ask) = asks.of(capability) {
        text += &format!("\nWhy: {}", ask.reason);
        if let Some(limit) = ask.capability.limit(&ask.scope.0) {
            text += &format!("\nLimited to: {limit}");
        }
    }
    let said = said.trim();
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

/// `head`, and under it each of `asks` with its reason.
fn listing(head: &str, asks: &[Asked]) -> String {
    let mut text = format!("{head}\nIt asks for:");
    for ask in asks {
        text +=
            &format!("\n{} - {}", ask.capability.name(), ask.reason);
        if let Some(limit) = ask.capability.limit(&ask.scope.0) {
            text += &format!(" [limited to {limit}]");
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
        info.name, info.version
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
    let petition =
        oversight::petition(world, &info.name, text, answers);
    world.entity_mut(petition).insert(subject);
}

/// Carries out the chief's answer to a petition about a plugin.
pub(super) fn answered(
    settled: On<Settled>,
    subjects: Query<&Subject>,
    mut commands: Commands,
) {
    let Ok(subject) = subjects.get(settled.entity) else {
        return;
    };
    let (subject, choice) = (subject.clone(), settled.choice);
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
