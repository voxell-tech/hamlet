//! What a plugin says about itself.

use bevy::ecs::component::Component;

/// A plugin that must be present, and the versions of it that do.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
pub struct Need {
    pub id: String,
    /// A version range such as `>=0.2` or `0.3`.
    pub range: String,
}

/// A plugin's id, name, version and the plugins it needs.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub needs: Vec<Need>,
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
}
