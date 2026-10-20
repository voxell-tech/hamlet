//! What a plugin may spend, as plain data from
//! `engine/assets/engine.toml`.

use bevy::ecs::resource::Resource;
use hamlet_engine_api::TreeLimits;
use serde::Deserialize;

use crate::tuning::{ENGINE, Tuning, TuningFile};

/// The caps on one plugin.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    /// Bytes of memory, all of the plugin's memories summed.
    pub memory: u64,
    /// Fuel for one call into the plugin.
    pub fuel_per_call: u64,
    /// Milliseconds one call may take.
    pub call_time: f32,
    /// Milliseconds of calls in one second.
    pub time_per_second: f32,
    /// Fuel of calls in one second.
    pub fuel_per_second: u64,
    /// Seconds in a row a plugin may go over a budget of a second
    /// before it is stopped.
    pub strikes: u32,
    /// Milliseconds of each frame shared among the plugins.
    pub frame_share: f32,
    /// Bytes copied into or out of the plugin by one call, checked
    /// before any copy.
    pub bytes_per_call: u64,
    pub calls_per_second: u32,
    pub events_per_second: u32,
    pub asset_writes_per_second: u32,
    pub panels: usize,
    /// Everything it adds: panels, commands, menu entries,
    /// shortcuts, settings pages and suggested workspaces.
    pub contributions: usize,
    /// Bytes of one asset.
    pub asset_bytes: u64,
    pub log_lines_per_second: u32,
    /// Bytes of one line of the log; a longer line is cut.
    pub log_line_bytes: usize,
    pub petitions_per_minute: u32,
    /// Widgets in one panel's tree.
    pub widgets: usize,
    /// How deep a panel's widgets may nest.
    pub widget_depth: usize,
    /// Bytes of text in one widget.
    pub widget_text: usize,
}

impl Tuning for Budgets {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("budgets");
}

impl Budgets {
    /// The most one panel's widget tree may hold.
    pub fn tree_limits(&self) -> TreeLimits {
        TreeLimits {
            nodes: self.widgets,
            depth: self.widget_depth,
            text: self.widget_text,
        }
    }
}
