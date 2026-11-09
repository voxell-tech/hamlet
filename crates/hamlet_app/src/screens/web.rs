//! The Web panels: each shows a web page under a bar of its address
//! (ADR 0014). The addresses are kept with the view in every build;
//! the panels are only in a macOS build with the `web` feature.

#[cfg(all(feature = "web", target_os = "macos"))]
mod agents;
#[cfg(all(feature = "web", target_os = "macos"))]
pub mod engine;
#[cfg(all(feature = "web", target_os = "macos"))]
mod fetch;
#[cfg(all(feature = "web", target_os = "macos"))]
mod keys;
#[cfg(all(feature = "web", target_os = "macos"))]
mod page;
#[cfg(all(feature = "web", target_os = "macos"))]
mod panel;

use std::collections::BTreeMap;

use bevy::app::App;
use bevy::ecs::resource::Resource;

/// The id of the first Web panel. The others are `web-2`, `web-3`
/// and so on.
pub const PANEL: &str = "web";

pub fn plugin(app: &mut App) {
    app.init_resource::<Addresses>()
        .init_resource::<AgentTabs>();
    #[cfg(all(feature = "web", target_os = "macos"))]
    {
        app.add_plugins((panel::plugin, fetch::plugin));
        // The engine panics on start without its framework, so it is
        // started only when both it and the helper are there.
        if engine::located().is_some() && page::helper().exists() {
            app.add_plugins((
                page::plugin,
                keys::plugin,
                agents::plugin,
            ));
        }
    }
}

/// The address each Web panel shows, by panel id, as the view keeps
/// it.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Addresses(pub BTreeMap<String, String>);

/// The Web panels that are agents' tabs, with the id of each one's
/// agent, by panel id, as the view keeps them.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct AgentTabs(pub BTreeMap<String, u64>);
