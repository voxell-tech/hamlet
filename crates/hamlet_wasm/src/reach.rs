//! What a plugin may reach of the others: the collections it imports
//! that are there, in the storage of the plugin that shares each.

use bevy::prelude::*;
use hamlet_sdk::capability::Uninstalled;
use hamlet_sdk::plugin::{Manifest, PluginInfo, Status, unlink};

use crate::host::{Reach, Shared};
use crate::limits::Limits;
use crate::reload::Watch;
use crate::storage::{Caps, DATA, Storage};

/// The plugins whose collections others may reach.
pub type Sharing<'w, 's> = Query<
    'w,
    's,
    (
        &'static PluginInfo,
        &'static Manifest,
        &'static Status,
        &'static Watch,
    ),
    Without<Uninstalled>,
>;

/// What the plugin of `manifest` may reach: the collections it
/// imports that a running plugin shares.
pub fn of(
    manifest: &Manifest,
    sharing: &Sharing,
    limits: &Limits,
) -> Reach {
    let shared = manifest
        .imports
        .collections
        .iter()
        .filter_map(|import| {
            let (plugin, name) = unlink(import)?;
            let (_, offered, _, watch) =
                sharing.iter().find(|(info, _, status, _)| {
                    info.id == plugin && **status == Status::Running
                })?;
            let collection = offered.exports.collection(name)?;
            Some(Shared {
                plugin: plugin.into(),
                name: name.into(),
                prefix: collection.prefix.clone(),
                writable: collection.writable,
                storage: Storage::new(
                    watch.folder.join(DATA),
                    Caps::of(limits),
                ),
            })
        })
        .collect();
    Reach {
        imports: manifest.imports.clone(),
        own: manifest.exports.collections.clone(),
        shared,
    }
}
