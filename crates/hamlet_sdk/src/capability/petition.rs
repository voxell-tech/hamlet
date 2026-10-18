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
use super::install::{self, Uninstalled};
use super::name::Capability;
use super::subject::Subject;
use crate::plugin::PluginInfo;

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
    raise(world, &info, subject, &head, &asks.0);
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
    raise(world, &info, subject, &head, &more);
}

fn raise(
    world: &mut World,
    info: &PluginInfo,
    subject: Subject,
    head: &str,
    asks: &[Asked],
) {
    let open = world
        .query::<&Subject>()
        .iter(world)
        .any(|open| *open == subject);
    if open {
        return;
    }
    let mut text = format!("{head}\nIt asks for:");
    for ask in asks {
        text +=
            &format!("\n{} - {}", ask.capability.name(), ask.reason);
    }
    let petition = oversight::petition(
        world,
        &info.name,
        text,
        vec![Answer::Allow, Answer::Deny],
    );
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
            let choice =
                if allow { Choice::Allow } else { Choice::Deny };
            for capability in capabilities {
                world.trigger(Decide {
                    plugin: plugin.clone(),
                    capability,
                    choice,
                    this_run: false,
                });
            }
            world.flush();
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
