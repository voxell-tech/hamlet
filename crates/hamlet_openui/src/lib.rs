//! Panels written in Hamlet's panel language: the library of
//! components, the evaluation of a program into a panel tree, and the
//! state and actions of the panel.

pub mod action;
pub mod components;
pub mod convert;
pub mod data;
pub mod draw;
pub mod evaluate;
pub mod interact;
pub mod library;
pub mod limits;
pub mod read;
pub mod source;
pub mod stale;
pub mod state;
pub mod wire;

#[cfg(test)]
mod tests;

use bevy::prelude::*;
use hamlet_assets::TuningPlugin;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::tree::store::PluginTrees;

pub use self::library::PanelLibrary;
use self::limits::LangTuning;
pub use self::limits::PanelLimits;
pub use self::source::Source;

/// Draws the panels that have a [`Source`] into the SDK's tree read
/// model. Add it after the SDK.
pub fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<LangTuning>::default())
        .init_resource::<AgentList>()
        .init_resource::<PluginTrees>()
        .init_resource::<PanelLimits>()
        .init_resource::<PanelLibrary>()
        .add_observer(source::compile)
        .add_observer(interact::interact)
        .add_observer(data::restart)
        .add_systems(
            Update,
            (
                stale::all.run_if(stale::reads_moved),
                draw::draw.in_set(Drawing).run_if(stale::due),
            )
                .chain(),
        );
}

/// The system that draws the panels written in the language.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Drawing;
