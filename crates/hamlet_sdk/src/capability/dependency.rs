//! What the petitions to install and to remove a plugin say of the
//! plugins it needs and the plugins that need it.

use bevy::ecs::entity::Entity;
use bevy::ecs::query::Has;
use bevy::ecs::world::World;

use super::install::Uninstalled;
use crate::plugin::needs::{self, Unmet};
use crate::plugin::{
    Manifest, Need, PluginInfo, Status, plain, present,
};

/// Lines on the plugins that `plugin` needs, or would like, that are
/// not installed, each starting on a new line. Empty where there are
/// none.
pub(super) fn needing(world: &mut World, plugin: Entity) -> String {
    let Some(manifest) = world.get::<Manifest>(plugin).cloned()
    else {
        return String::new();
    };
    let mut plugins =
        world.query::<(&PluginInfo, &Status, Has<Uninstalled>)>();
    let present = present(plugins.iter(world).map(
        |(info, status, uninstalled)| (info, *status, uninstalled),
    ));
    let mut text = String::new();
    // The ids and ranges are the manifest's, so each is one plain
    // line.
    for unmet in needs::lacking(&manifest.needs, &present) {
        text += &format!("\nIt {}.", plain(&unmet.to_string()));
    }
    for unmet in needs::lacking(&manifest.wants, &present) {
        text += &format!(
            "\nIt works better with `{}`, which is not installed.",
            plain(id_of(&unmet))
        );
    }
    text
}

fn id_of(unmet: &Unmet) -> &str {
    match unmet {
        Unmet::Missing(id)
        | Unmet::NotInstalled(id)
        | Unmet::Off(id, _)
        | Unmet::Version { id, .. } => id,
    }
}

/// A line naming the plugins that need `plugin`, directly or through
/// one another, which go off with it, starting on a new line. Empty
/// where none does.
pub(super) fn needed(world: &mut World, plugin: Entity) -> String {
    let Some(id) =
        world.get::<PluginInfo>(plugin).map(|info| info.id.clone())
    else {
        return String::new();
    };
    let mut plugins =
        world.query::<(&PluginInfo, &Manifest, Has<Uninstalled>)>();
    let all = plugins
        .iter(world)
        .filter(|(.., uninstalled)| !uninstalled)
        .map(|(info, manifest, _)| {
            (info.name.as_str(), info.id.as_str(), &manifest.needs)
        })
        .collect::<Vec<_>>();
    let edges = all
        .iter()
        .map(|(_, id, needs)| (*id, needs.as_slice()))
        .collect::<Vec<(&str, &[Need])>>();
    let names = needs::dependents(&id, &edges)
        .into_iter()
        .filter_map(|found| {
            all.iter()
                .find(|(_, id, _)| *id == found)
                .map(|(name, ..)| plain(name))
        })
        .collect::<Vec<_>>();
    if names.is_empty() {
        return String::new();
    }
    format!("\nSwitched off with it: {}.", names.join(", "))
}
