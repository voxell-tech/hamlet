//! What a panel written in the language may cost. The values are in
//! `assets/panel_lang.toml`.

use bevy::prelude::*;
use hamlet_assets::{File, Tuning, designed, follow};

/// The limits on parsing, evaluating and drawing one panel.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelLimits {
    /// The nesting of expressions, and of statements inlined through
    /// references.
    pub max_depth: usize,
    /// The nodes of the program after references are inlined.
    pub max_nodes: usize,
    /// The steps of one evaluation.
    pub max_steps: usize,
    /// The bytes of strings built in one evaluation.
    pub max_string_bytes: usize,
    /// The nodes the panel's tree may hold. The rest are dropped.
    pub tree_nodes: usize,
    /// The levels the panel's tree may nest. The rest are dropped.
    pub tree_depth: usize,
}

impl PanelLimits {
    /// The limits the language's runtime is given.
    pub fn runtime(&self) -> openui_lang::Limits {
        let mut limits = openui_lang::Limits::DEFAULT;
        limits.max_depth = self.max_depth;
        limits.max_nodes = self.max_nodes;
        limits.max_steps = self.max_steps;
        limits.max_string_bytes = self.max_string_bytes;
        limits
    }
}

/// The sections of `assets/panel_lang.toml`.
#[derive(Asset, TypePath, Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LangTuning {
    pub limits: PanelLimits,
}

impl Tuning for LangTuning {
    const FILE: File = hamlet_assets::PANEL_LANG;

    fn apply(&self, was: &Self, world: &mut World) {
        follow(world, &was.limits, &self.limits);
    }
}

designed!(LangTuning { PanelLimits => limits });
