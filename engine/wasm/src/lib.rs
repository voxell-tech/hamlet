//! The host of WASM plugins: wasmtime with the component model, a
//! store for each plugin, and each call into a plugin held to its
//! budgets. The only engine crate that knows wasmtime.

mod add;
mod budget;
mod folder;
mod host;
mod instance;
mod mail;
mod nodes;
mod panel;
mod reload;
mod retained;
mod run;
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
use hamlet_engine_plugins::{ContributedBy, PluginInfo, Stopped};

pub use self::add::AddFrom;
use self::budget::Spending;
use self::folder::look;
pub use self::folder::{MANIFEST, Package, WASM};
pub use self::host::{Host, WasmTuning};
pub use self::instance::{Caps, Fault, Instance, NodeInput, Spent};
use self::nodes::NodeChange;
pub use self::panel::{Drawn, Input, Shows};
use self::panel::{Inbox, Moving, Redraw};
use self::reload::{Loaded, Reloading};
pub use self::retained::{NodeAction, NodeOf, NodeStats, UiRoot};
use self::retained::{WasmWorldExt, Widgets};

pub fn plugin(app: &mut App) {
    app.add_plugins((
        host::plugin,
        folder::plugin,
        add::plugin,
        run::plugin,
        panel::plugin,
        retained::plugin,
        reload::plugin,
    ));
}

/// The WASM host's methods on [`Commands`].
pub(crate) trait WasmCommandsExt {
    /// Adds the plugin installed in `folder`, off.
    fn spawn_plugin(
        &mut self,
        manifest: Manifest,
        folder: PathBuf,
    ) -> Entity;

    /// Has the WASM plugin draw `surface`, its panel or settings page
    /// `id`.
    fn adopt_surface(&mut self, surface: Entity, id: &PanelId);

    /// Stops a plugin at fault and tells the person why.
    fn stop_plugin(
        &mut self,
        log: &mut MessageWriter<LogLine>,
        plugin: Entity,
        info: &PluginInfo,
        fault: &Fault,
    );

    /// Changes `plugin`'s widgets as its nodes were changed, once the
    /// commands are applied. `root` is a node the host made, with the
    /// widget it stands for.
    fn apply_node_changes(
        &mut self,
        plugin: Entity,
        root: Option<(u32, Entity)>,
        changes: Vec<NodeChange>,
    );

    /// Takes away every widget of `plugin`'s nodes but its roots.
    fn forget_widgets(&mut self, plugin: Entity, widgets: &Widgets);

    /// Has a reloaded plugin build its retained panels again: its
    /// widgets are taken away, and each panel shown is offered anew.
    fn remount(
        &mut self,
        plugin: Entity,
        widgets: Option<&Widgets>,
        roots: &Query<(Entity, &UiRoot, &ContributedBy)>,
    );
}

impl WasmCommandsExt for Commands<'_, '_> {
    fn spawn_plugin(
        &mut self,
        manifest: Manifest,
        folder: PathBuf,
    ) -> Entity {
        let loaded = Loaded {
            wasm: look(&folder.join(WASM)),
            manifest: look(&folder.join(MANIFEST)),
        };
        self.spawn((
            PluginInfo::of(manifest, false),
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
        self.entity(surface).insert((
            Drawn {
                name,
                shows: Shows::Waiting,
            },
            Redraw,
        ));
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

    fn forget_widgets(&mut self, plugin: Entity, widgets: &Widgets) {
        for built in widgets.0.values() {
            if let Some(holder) = built.holder {
                self.entity(built.entity).try_despawn();
                self.entity(holder).try_despawn();
            }
        }
        self.entity(plugin).try_remove::<Widgets>();
    }

    fn remount(
        &mut self,
        plugin: Entity,
        widgets: Option<&Widgets>,
        roots: &Query<(Entity, &UiRoot, &ContributedBy)>,
    ) {
        if let Some(widgets) = widgets {
            self.forget_widgets(plugin, widgets);
        }
        for (surface, root, by) in roots {
            if by.0 == plugin {
                self.entity(surface).insert(*root);
            }
        }
    }
}
