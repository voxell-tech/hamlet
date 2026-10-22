//! What plugins may spend: the engine's caps, `[budgets]`, the same
//! for every plugin, and the limits of each plugin's WASM instance,
//! `[limits]` in `engine/tuning/engine.toml` unless the plugin or the
//! person gives another.

use core::ops::Deref;

use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;
use hamlet_engine_api::{Limits, PluginLimits};
use serde::Deserialize;

use crate::tuning::{ENGINE, Tuning, TuningFile};

/// The engine's caps, each held by every plugin alike.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    /// Seconds in a row a plugin may go over a budget of a second
    /// before it is stopped.
    pub strikes: u32,
    /// Milliseconds of each frame shared among the plugins.
    pub frame_share: f32,
    /// Milliseconds one call into a plugin may take, held by the
    /// epoch; a call past it is stopped, and its plugin with it.
    pub call_time: f32,
    /// Plugins a chain of calls and events may cross.
    pub chain_depth: u32,
    /// Bytes copied into or out of a plugin by one call, checked
    /// before any copy.
    pub bytes_per_call: u64,
    pub calls_per_second: u32,
    pub events_per_second: u32,
    /// Requests to the engine's services one call may make; past
    /// them a request is not taken.
    pub requests_per_call: usize,
    /// Resources of the system interface (WASI) a plugin holds at
    /// once, such as streams and timers; past them one is refused.
    pub wasi_resources: usize,
    /// Assets made, changed or deleted in one second.
    pub asset_writes_per_second: u32,
    /// Calls of other plugins' services in one second.
    pub service_calls_per_second: u32,
    /// Events a plugin sends in one second; the rest are refused.
    pub events_sent_per_second: u32,
    pub fetches_per_minute: u32,
    /// Fetches under way at once.
    pub fetches_waiting: usize,
    /// Bytes of the answer to one fetch.
    pub fetch_bytes: u64,
    /// Seconds one fetch may take, its lookup with it.
    pub fetch_time: f32,
    pub panels: usize,
    /// Everything a plugin adds: panels, commands, menu entries,
    /// shortcuts, settings pages and suggested workspaces.
    pub contributions: usize,
    /// Bytes of one asset. A read asset larger than `bytes_per_call`
    /// cannot be handed over.
    pub asset_bytes: u64,
    /// Assets a plugin may own.
    pub assets: usize,
    pub log_lines_per_second: u32,
    /// Bytes of one line of the log; a longer line is cut.
    pub log_line_bytes: usize,
    pub petitions_per_minute: u32,
    /// Nodes of retained panels made, set or put in order in one
    /// call; past them the call fails.
    pub node_changes_per_call: usize,
    /// The same, in one second.
    pub node_changes_per_second: u32,
    /// Nodes of retained panels a plugin holds at once; past them
    /// the call fails.
    pub nodes: usize,
    /// How deep a node of a retained panel may sit under its root;
    /// a call that makes one deeper fails.
    pub node_depth: usize,
    /// Entities of a plugin's scenes held at once, each copy of
    /// instances counted; past them a make fails.
    pub scene_entities: usize,
    /// Vertices of a plugin's meshes held at once.
    pub scene_vertices: usize,
    pub scene_materials: usize,
    pub scene_lights: usize,
    pub scene_cameras: usize,
    /// Changes to a plugin's scenes in one call; past them the call
    /// fails.
    pub scene_changes_per_call: usize,
    /// The same, in one second; past them the plugin waits.
    pub scene_changes_per_second: u32,
    /// Bytes of GPU memory a plugin's meshes, textures and buffers
    /// take, as the engine counts them; past them a make fails.
    pub gpu_memory: u64,
    /// Shaders, buffers, textures, compute and passes a plugin holds
    /// at once.
    pub gpu_objects: usize,
    /// Bytes of one shader's source.
    pub shader_bytes: usize,
    /// Workgroups a plugin's compute runs in one frame; a dispatch
    /// past them is refused, and runs past them wait for the next
    /// frame.
    pub workgroups_per_frame: u32,
    /// Milliseconds of GPU time a plugin's compute and passes take
    /// in one second, where the GPU can time them; past them
    /// they wait for the next second.
    pub gpu_time_per_second: f32,
}

impl Tuning for Budgets {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("budgets");

    fn check(&self) -> Result<(), String> {
        let times = [
            ("frame_share", self.frame_share),
            ("call_time", self.call_time),
            ("fetch_time", self.fetch_time),
            ("gpu_time_per_second", self.gpu_time_per_second),
        ];
        match times.iter().find(|(_, x)| !(x.is_finite() && *x > 0.0))
        {
            Some((name, _)) => {
                Err(format!("{name} is not a number above zero"))
            }
            None => Ok(()),
        }
    }
}

/// The limits of a plugin that asks for none and the person did not
/// change.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(transparent)]
pub struct DefaultLimits(pub PluginLimits);

impl Tuning for DefaultLimits {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("limits");

    fn check(&self) -> Result<(), String> {
        self.0
            .check()
            .map_err(|limit| format!("{limit} is not above zero"))
    }
}

impl Deref for DefaultLimits {
    type Target = PluginLimits;

    fn deref(&self) -> &PluginLimits {
        &self.0
    }
}

/// On a plugin: the limits its instance is held to.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct PluginBudgets(pub PluginLimits);

impl Deref for PluginBudgets {
    type Target = PluginLimits;

    fn deref(&self) -> &PluginLimits {
        &self.0
    }
}

impl PluginBudgets {
    /// Each limit as the person set it, else as the plugin `asked`,
    /// else as the engine has it.
    pub fn resolve(
        defaults: &DefaultLimits,
        asked: &Limits,
        chosen: Option<&Limits>,
    ) -> Self {
        let mut limits = defaults.with(asked);
        if let Some(chosen) = chosen {
            limits = limits.with(chosen);
        }
        Self(limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_person_comes_before_the_plugin_before_the_engine() {
        let defaults = DefaultLimits::designed();
        let memory = |memory| Limits { memory };
        let used = |asked, chosen: Option<&Limits>| {
            PluginBudgets::resolve(&defaults, &memory(asked), chosen)
                .memory
        };
        assert_eq!(used(None, None), defaults.memory);
        assert_eq!(used(Some(2), None), 2);
        assert_eq!(used(Some(2), Some(&memory(Some(3)))), 3);
        assert_eq!(used(Some(2), Some(&memory(None))), 2);
    }

    #[test]
    fn a_call_time_not_above_zero_is_refused() {
        for bad in ["nan", "inf", "0.0", "-1.0"] {
            let text = ENGINE.text.replace(
                "call_time = 1_000.0",
                &format!("call_time = {bad}"),
            );
            assert!(
                crate::tuning::read::<Budgets>(&text).is_err(),
                "call_time = {bad} was taken"
            );
        }
    }
}
