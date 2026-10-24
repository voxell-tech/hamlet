//! The host of WASM plugins: wasmtime with the component model, a
//! store for each plugin, and each call into a plugin held only to
//! the limits the person switched on and the guards. The only engine
//! crate that knows wasmtime.

mod add;
mod audio;
mod budget;
mod compiled;
mod dialog;
mod folder;
mod host;
mod instance;
mod mail;
mod nodes;
mod panel;
mod reload;
mod retained;
mod run;
mod scene;
mod surfaces;
mod wasi;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::time::Instant;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use hamlet_engine_api::{Manifest, PanelId};
use hamlet_engine_core::LogLine;
use hamlet_engine_plugins::{
    ContributedBy, PluginInfo, PluginKind, Stopped,
};

pub use crate::add::AddFrom;
use crate::budget::Spending;
pub use crate::compiled::SHIPPED;
pub(crate) use crate::folder::PluginPathExt;
pub use crate::folder::{MANIFEST, Package, WASM};
pub use crate::host::{Host, WasmTuning, precompile};
pub use crate::instance::{Caps, Fault, Instance, NodeInput, Spent};
use crate::nodes::NodeChange;
pub use crate::nodes::NodeError;
pub use crate::panel::Surface;
use crate::panel::{Inbox, Moving};
use crate::reload::{Loaded, Reloading};
use crate::retained::{Holders, Kids, WasmWorldExt};
pub use crate::retained::{NodeAction, NodeOf, NodeStats, UiRoot};
pub use crate::run::Compiling;
pub use crate::scene::{Powers, SceneCaps, SceneError, ShowsCamera};
use crate::surfaces::SurfaceChange;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        host::plugin,
        folder::plugin,
        add::plugin,
        dialog::plugin,
        run::plugin,
        panel::plugin,
        retained::plugin,
        reload::plugin,
        scene::plugin,
        audio::plugin,
    ));
}

/// The WASM host's methods on [`Commands`].
pub(crate) trait WasmCommandsExt {
    /// Adds the plugin of `kind` in `folder`, off.
    fn spawn_plugin(
        &mut self,
        manifest: Manifest,
        folder: PathBuf,
        kind: PluginKind,
    ) -> Entity;

    /// Has the WASM plugin build `surface`, its panel or settings
    /// page `id`.
    fn adopt_surface(&mut self, surface: Entity, id: &PanelId);

    /// Stops a plugin at fault and tells the person why.
    fn stop_plugin(
        &mut self,
        log: &mut MessageWriter<LogLine>,
        plugin: Entity,
        info: &PluginInfo,
        fault: &Fault,
    );

    /// Changes the holders of `plugin`'s nodes as its nodes were
    /// changed, once the commands are applied. `root` is a node the
    /// host made, with its holder.
    fn apply_node_changes(
        &mut self,
        plugin: Entity,
        root: Option<(u32, Entity)>,
        changes: Vec<NodeChange>,
    );

    /// Changes what `plugin` shows outside its panels as it changed
    /// it, once the commands are applied.
    fn apply_surface_changes(
        &mut self,
        plugin: Entity,
        changes: Vec<SurfaceChange>,
    );

    /// Takes away every node of `plugin` but its roots, which are
    /// left holding nothing.
    fn forget_nodes(&mut self, plugin: Entity, holders: &Holders);

    /// Has a reloaded plugin build its panels and settings pages
    /// again: its nodes are taken away, and each one shown is offered
    /// anew.
    fn remount(
        &mut self,
        plugin: Entity,
        holders: Option<&Holders>,
        roots: &Query<(Entity, &UiRoot, &ContributedBy)>,
    );
}

impl WasmCommandsExt for Commands<'_, '_> {
    fn spawn_plugin(
        &mut self,
        manifest: Manifest,
        folder: PathBuf,
        kind: PluginKind,
    ) -> Entity {
        let loaded = Loaded {
            wasm: folder.join(WASM).look(),
            manifest: folder.join(MANIFEST).look(),
        };
        self.spawn((
            PluginInfo::of(manifest, kind),
            Package { folder },
            loaded,
        ))
        .id()
    }

    fn adopt_surface(&mut self, surface: Entity, id: &PanelId) {
        let name = id
            .as_str()
            .split_once('/')
            .map_or_else(String::new, |(_, name)| name.to_owned());
        self.entity(surface).insert(Surface {
            name,
            stopped: None,
        });
    }

    fn stop_plugin(
        &mut self,
        log: &mut MessageWriter<LogLine>,
        plugin: Entity,
        info: &PluginInfo,
        fault: &Fault,
    ) {
        self.entity(plugin)
            .remove::<(Instance, Inbox, Spending, Reloading, Moving)>(
            )
            .insert(Stopped(fault.to_string()));
        log.write(LogLine::error(
            info.id.as_str(),
            format!("{} was stopped: {fault}", info.name),
        ));
    }

    fn apply_node_changes(
        &mut self,
        plugin: Entity,
        root: Option<(u32, Entity)>,
        changes: Vec<NodeChange>,
    ) {
        if changes.is_empty() && root.is_none() {
            return;
        }
        self.queue(move |world: &mut World| {
            let started = Instant::now();
            world.build_nodes(plugin, root, changes);
            if let Some(mut stats) =
                world.get_resource_mut::<NodeStats>()
            {
                stats.apply_time += started.elapsed();
            }
        });
    }

    fn apply_surface_changes(
        &mut self,
        plugin: Entity,
        changes: Vec<SurfaceChange>,
    ) {
        if changes.is_empty() {
            return;
        }
        self.queue(move |world: &mut World| {
            world.change_surfaces(plugin, changes);
        });
    }

    fn forget_nodes(&mut self, plugin: Entity, holders: &Holders) {
        for holder in holders.0.values() {
            if holder.root {
                self.entity(holder.entity)
                    .try_insert(Kids::default());
            } else {
                self.entity(holder.entity).try_despawn();
            }
        }
        self.entity(plugin).try_remove::<Holders>();
    }

    fn remount(
        &mut self,
        plugin: Entity,
        holders: Option<&Holders>,
        roots: &Query<(Entity, &UiRoot, &ContributedBy)>,
    ) {
        if let Some(holders) = holders {
            self.forget_nodes(plugin, holders);
        }
        for (surface, root, by) in roots {
            if by.0 == plugin {
                self.entity(surface).insert(*root);
            }
        }
    }
}
