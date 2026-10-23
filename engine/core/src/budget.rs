//! What keeps a plugin from crashing or freezing the engine,
//! `[guards]` in `engine/tuning/engine.toml`, and the limits the
//! person holds each plugin to, none until they switch one on.

use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;
use hamlet_engine_api::{LimitKind, LimitUnit, PluginLimits};
use serde::Deserialize;

use crate::tuning::{ENGINE, Tuning, TuningFile};

/// The same for every plugin. None is a limit on what a plugin
/// spends: each keeps one plugin from crashing or freezing the
/// engine.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guards {
    /// How deep a node of a retained panel may sit under its root; a
    /// call that makes one deeper fails. The layout of the interface
    /// is recursive, and a tree deeper than this would overflow its
    /// stack.
    pub node_depth: usize,
    /// Seconds one call may hold the frame while its plugin has no
    /// call time switched on, before the plugin is stopped as hung.
    /// While a call holds the frame nothing else reaches the engine,
    /// so it cannot be stopped by hand.
    pub hung: f32,
}

impl Tuning for Guards {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("guards");

    fn check(&self) -> Result<(), String> {
        if self.hung.is_finite() && self.hung > 0.0 {
            Ok(())
        } else {
            Err("hung is not a number above zero".into())
        }
    }
}

/// The value each limit starts at when the person first switches it
/// on, `[limits]` in `engine.toml`.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(transparent)]
pub struct StartingLimits(pub PluginLimits);

impl Tuning for StartingLimits {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("limits");
}

impl StartingLimits {
    /// The value `kind` starts at: the file's, else one of its unit.
    pub fn of(&self, kind: LimitKind) -> u64 {
        self.0.get(kind).filter(|value| *value > 0).unwrap_or(
            match kind.unit() {
                LimitUnit::Bytes => 1 << 30,
                LimitUnit::Milliseconds => 1_000,
                LimitUnit::Count => 10_000,
            },
        )
    }
}

/// On a plugin: the limits the person switched on for it.
#[derive(Component, Clone, Copy, PartialEq, Default, Debug)]
pub struct ActiveLimits(pub PluginLimits);
