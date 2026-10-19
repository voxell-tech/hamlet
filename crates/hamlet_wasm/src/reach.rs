//! What a plugin may reach of the others: the collections it imports
//! that are there, in the storage of the plugin that shares each, and
//! the plugins that implement the interfaces it imports.

use bevy::prelude::*;
use hamlet_sdk::capability::{Capability, Granted, Uninstalled};
use hamlet_sdk::plugin::{
    Imported, Manifest, PluginInfo, Status, unlink,
};

use crate::host::{Reach, Shared};
use crate::limits::Limits;
use crate::reload::Watch;
use crate::storage::{Caps, DATA, Storage};

/// The plugins whose collections and interfaces others may reach.
pub type Sharing<'w, 's> = Query<
    'w,
    's,
    (
        &'static PluginInfo,
        &'static Manifest,
        &'static Status,
        &'static Watch,
        &'static Granted,
    ),
    Without<Uninstalled>,
>;

/// What the plugin of `manifest` may reach: the collections it
/// imports that a running plugin shares, and the plugins that
/// implement each interface it imports.
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
            let (_, offered, _, watch, _) =
                sharing.iter().find(|(info, _, status, ..)| {
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
    let offering = sharing
        .iter()
        .filter(|(info, _, status, _, granted)| {
            info.id != manifest.id
                && **status == Status::Running
                && granted.holds(Capability::PluginExport)
        })
        .map(|(info, offered, ..)| (info.id.as_str(), offered))
        .collect::<Vec<_>>();
    let implementers = manifest
        .imports
        .interfaces
        .iter()
        .map(|imported| {
            (imported.name.clone(), implementers(imported, &offering))
        })
        .collect();
    Reach {
        imports: manifest.imports.clone(),
        own: manifest.exports.collections.clone(),
        shared,
        implementers,
    }
}

/// The ids of the plugins of `offering` that implement `imported`,
/// in order: those it may be imported from that export it with the
/// functions it names, of the same types.
fn implementers(
    imported: &Imported,
    offering: &[(&str, &Manifest)],
) -> Vec<String> {
    let mut ids = offering
        .iter()
        .filter(|(id, manifest)| {
            imported.from(id)
                && manifest
                    .exports
                    .interface(&imported.name)
                    .is_some_and(|exported| {
                        imported.unlike(exported).is_none()
                    })
        })
        .map(|(id, _)| id.to_string())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use hamlet_sdk::plugin::{Function, Interface};

    use super::*;

    fn exporting(id: &str, result: &str) -> Manifest {
        let mut manifest = Manifest::built_in(id);
        manifest.exports.interfaces.push(Interface {
            name: "tide-table".into(),
            functions: vec![Function {
                name: "height".into(),
                params: vec!["string".into()],
                result: Some(result.into()),
            }],
        });
        manifest
    }

    #[test]
    fn the_implementers_are_the_plugins_that_export_it_alike() {
        let mut imported = Imported {
            name: "tide-table".into(),
            from: Vec::new(),
            functions: exporting("", "option<f64>")
                .exports
                .interfaces[0]
                .functions
                .clone(),
        };
        let (tides, almanac, odd) = (
            exporting("tides", "option<f64>"),
            exporting("almanac", "option< f64 >"),
            exporting("odd", "f64"),
        );
        let offering =
            [("tides", &tides), ("odd", &odd), ("almanac", &almanac)];

        assert_eq!(
            implementers(&imported, &offering),
            ["almanac", "tides"]
        );
        imported.from = vec!["tides".into()];
        assert_eq!(implementers(&imported, &offering), ["tides"]);
    }
}
