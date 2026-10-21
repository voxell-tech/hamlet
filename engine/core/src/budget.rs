//! What plugins may spend: the engine's own caps, `[budgets]`, and
//! the limits each plugin is held to, `[limits]` in
//! `engine/tuning/engine.toml` unless the plugin or the person gives
//! another.

use core::ops::Deref;

use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;
use hamlet_engine_api::{Limits, PluginLimits};
use serde::Deserialize;

use crate::tuning::{ENGINE, Tuning, TuningFile};

/// The caps on all plugins together.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    /// Seconds in a row a plugin may go over a limit of a second
    /// before it is stopped.
    pub strikes: u32,
    /// Milliseconds of each frame shared among the plugins.
    pub frame_share: f32,
    /// Plugins a chain of calls and events may cross.
    pub chain_depth: u32,
}

impl Tuning for Budgets {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("budgets");
}

/// The limits of a plugin that asks for none and the person did not
/// change.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(transparent)]
pub struct DefaultLimits(pub PluginLimits);

impl Tuning for DefaultLimits {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("limits");
}

impl Deref for DefaultLimits {
    type Target = PluginLimits;

    fn deref(&self) -> &PluginLimits {
        &self.0
    }
}

/// On a plugin: the limits it is held to.
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
    use hamlet_engine_api::Limit;

    use super::*;

    #[test]
    fn the_person_comes_before_the_plugin_before_the_engine() {
        let defaults = DefaultLimits::designed();
        let asked = Limits {
            fuel_per_call: Some(2_000_000_000),
            call_time: Some(2_000.0),
            ..Limits::default()
        };
        let mut chosen = Limits::default();
        chosen.set(Limit::CallTime, Limit::CallTime.parse("3_000"));
        let used =
            PluginBudgets::resolve(&defaults, &asked, Some(&chosen));
        assert_eq!(used.memory, defaults.memory);
        assert_eq!(used.fuel_per_call, 2_000_000_000);
        assert_eq!(used.call_time, 3_000.0);
    }
}
