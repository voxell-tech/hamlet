//! The shortcuts in force, and which of them run.

use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Added, Changed, Or};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy_fynix::shortcut::Chord;

use super::Key;
use crate::capability::{Capability, Granted, allowed};
use crate::command::{Command, full};
use crate::plugin::{ContributedBy, PluginInfo};

/// Every shortcut that may show.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Keys(pub Vec<Bound>);

/// A chord bound to a command.
#[derive(Clone, PartialEq, Debug)]
pub struct Bound {
    pub chord: Chord,
    /// The id [`RunCommand`](crate::command::RunCommand) takes.
    pub command: String,
    /// The id of the plugin that added it.
    pub plugin: String,
    pub builtin: bool,
    pub standing: Standing,
}

/// Whether a chord runs its command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Standing {
    Runs,
    /// A built-in plugin has the chord, so this one never runs.
    Shadowed,
    /// Another binding of the same kind has the chord, and neither
    /// runs.
    Clash,
}

pub(super) fn moved(
    changed: Query<
        (),
        Or<(
            Added<Key>,
            Added<Command>,
            Changed<Granted>,
            Changed<PluginInfo>,
        )>,
    >,
    mut keys: RemovedComponents<Key>,
    mut commands: RemovedComponents<Command>,
) -> bool {
    !changed.is_empty()
        || keys.read().count() > 0
        || commands.read().count() > 0
}

pub(super) fn keep(
    keys: Query<(&Key, &ContributedBy)>,
    commands: Query<(&Command, &ContributedBy)>,
    plugins: Query<(&PluginInfo, Option<&Granted>)>,
    mut bound: ResMut<Keys>,
) {
    let mut now = keys
        .iter()
        .filter_map(|(key, by)| {
            let (info, granted) = plugins.get(by.0).ok()?;
            if !allowed(info, granted, Capability::UiKeys) {
                return None;
            }
            let (command, _) =
                commands.iter().find(|(command, of)| {
                    of.0 == by.0 && command.id == key.command
                })?;
            Some(Bound {
                chord: key.chord,
                command: full(&info.id, &command.id),
                plugin: info.id.clone(),
                builtin: info.builtin,
                standing: Standing::Runs,
            })
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| {
        (a.chord.to_string(), &a.command)
            .cmp(&(b.chord.to_string(), &b.command))
    });
    let held = now.clone();
    for one in &mut now {
        let same = |other: &&Bound| other.chord == one.chord;
        let builtins =
            held.iter().filter(same).filter(|b| b.builtin).count();
        let plugins =
            held.iter().filter(same).filter(|b| !b.builtin).count();
        one.standing = match (one.builtin, builtins, plugins) {
            (true, 1, _) => Standing::Runs,
            (true, ..) => Standing::Clash,
            (false, 0, 1) => Standing::Runs,
            (false, 0, _) => Standing::Clash,
            (false, ..) => Standing::Shadowed,
        };
    }
    if bound.0 != now {
        bound.0 = now;
    }
}
