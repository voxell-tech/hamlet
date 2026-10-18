//! A plugin's first use of a capability the chief has not decided.

use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::observer::On;
use bevy::ecs::query::Without;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;

use super::granted::{Granted, Verdict};
use super::install::Uninstalled;
use super::name::Capability;
use super::petition;
use super::subject::Subject;
use crate::plugin::PluginInfo;

/// A plugin tried to use a capability, or asked for it with the
/// reason it gives. If the manifest names it and the chief is yet to
/// decide, the chief is asked, once at a time for each plugin and
/// capability.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct Wanted {
    pub plugin: Entity,
    pub capability: Capability,
    /// The plugin's own reason, if it gave one.
    pub reason: String,
}

pub(super) fn wanted(
    wanted: On<Wanted>,
    plugins: Query<(&PluginInfo, &Granted), Without<Uninstalled>>,
    open: Query<&Subject>,
    mut commands: Commands,
) {
    let Ok((info, granted)) = plugins.get(wanted.plugin) else {
        return;
    };
    let capability = wanted.capability;
    if granted.verdict(capability) != Some(Verdict::Ask) {
        return;
    }
    let subject = Subject::Use {
        plugin: info.id.clone(),
        capability,
    };
    if open.iter().any(|open| *open == subject) {
        return;
    }
    let (plugin, reason) = (wanted.plugin, wanted.reason.clone());
    commands.queue(move |world: &mut World| {
        petition::using(world, plugin, capability, &reason)
    });
}
