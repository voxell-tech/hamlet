//! A plugin held back, or switched off, because a plugin it needs is
//! not there, not the right version or not running.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Added, Changed, Has, Or};
use bevy::ecs::system::{Commands, Query};
use bevy::log::{info, warn};

use super::needs::{Present, unmet};
use super::{Disable, Enable, Manifest, PluginInfo, Status};
use crate::capability::Uninstalled;

/// On the entity of a plugin that is not started, or was switched
/// off, for what it needs: the reason, as Settings shows it.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct Blocked(pub String);

/// Every plugin as the needs of the others find it.
pub fn present<'a>(
    plugins: impl Iterator<Item = (&'a PluginInfo, Status, bool)>,
) -> Vec<Present<'a>> {
    plugins
        .map(|(info, status, uninstalled)| Present {
            id: &info.id,
            version: &info.version,
            status: (!uninstalled).then_some(status),
        })
        .collect()
}

/// Whether a plugin came, went, changed or was switched.
pub(super) fn moved(
    changed: Query<
        (),
        Or<(Changed<Status>, Changed<Manifest>, Added<Uninstalled>)>,
    >,
    mut gone: RemovedComponents<PluginInfo>,
    mut installed: RemovedComponents<Uninstalled>,
) -> bool {
    !changed.is_empty()
        || gone.read().count() > 0
        || installed.read().count() > 0
}

/// Switches off the running plugins whose needs are no longer met,
/// with the reason, and switches on those held back whose needs are.
pub(super) fn reconcile(
    plugins: Query<(
        Entity,
        &PluginInfo,
        &Manifest,
        &Status,
        Has<Uninstalled>,
        Option<&Blocked>,
    )>,
    mut commands: Commands,
) {
    let present = present(plugins.iter().map(
        |(_, info, _, status, uninstalled, _)| {
            (info, *status, uninstalled)
        },
    ));
    for (entity, info, manifest, status, uninstalled, blocked) in
        &plugins
    {
        if uninstalled {
            continue;
        }
        match (unmet(&manifest.needs, &present), blocked) {
            (Some(why), _) => {
                let why = why.to_string();
                if matches!(
                    status,
                    Status::Starting | Status::Running
                ) {
                    warn!("plugin {}: switched off, {why}", info.id);
                    commands.trigger(Disable(info.id.clone()));
                }
                if blocked.is_none_or(|held| held.0 != why) {
                    commands.entity(entity).insert(Blocked(why));
                }
            }
            (None, Some(_)) => {
                commands.entity(entity).remove::<Blocked>();
                if *status == Status::Disabled {
                    info!("plugin {}: its needs are met", info.id);
                    commands.trigger(Enable(info.id.clone()));
                }
            }
            (None, None) => {}
        }
    }
}
