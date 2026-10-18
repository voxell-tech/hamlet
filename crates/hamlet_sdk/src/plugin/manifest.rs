//! What a plugin says about itself.

use bevy::ecs::component::Component;

use super::offers::{Exports, Imports};
use crate::capability::Capability;

/// A plugin that must be present, and the versions of it that do.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
pub struct Need {
    pub id: String,
    /// A version range such as `>=0.2` or `0.3`, as
    /// [`Range`](super::Range) reads it.
    pub range: String,
}

/// A plugin's id, name, version, the plugins it needs and what it
/// offers and uses.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub needs: Vec<Need>,
    /// Plugins that make it better and that it works without.
    pub wants: Vec<Need>,
    pub exports: Exports,
    pub imports: Imports,
    /// Compiled into the app, with everything granted.
    pub builtin: bool,
}

impl Manifest {
    /// The manifest of a plugin compiled into the app. Its name is
    /// its id and its version the workspace's.
    pub fn built_in(id: &str) -> Self {
        Self {
            id: id.into(),
            name: id.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            needs: Vec::new(),
            wants: Vec::new(),
            exports: Exports::default(),
            imports: Imports::default(),
            builtin: true,
        }
    }

    pub fn named(mut self, name: &str) -> Self {
        self.name = name.into();
        self
    }

    pub fn version(mut self, version: &str) -> Self {
        self.version = version.into();
        self
    }

    /// Needs the plugin `id`, in a version in `range`.
    pub fn needs(mut self, id: &str, range: &str) -> Self {
        self.needs.push(Need {
            id: id.into(),
            range: range.into(),
        });
        self
    }

    /// What `capability` is chosen for one by one: the plugins this
    /// one imports from for `plugin.use`. Empty for a capability that
    /// is not scoped.
    pub fn targets(&self, capability: Capability) -> Vec<String> {
        match capability {
            Capability::PluginUse => self
                .imports
                .plugins()
                .into_iter()
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        }
    }
}
