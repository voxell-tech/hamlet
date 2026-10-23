//! The bundle: the folder of the plugins that come with Hamlet, built
//! for `wasm32-wasip2` by `scripts/build_plugins.sh`, each in a
//! folder of its own with its `plugin.toml` and `plugin.wasm`.

use std::env;
use std::path::{Path, PathBuf};

use bevy::ecs::resource::Resource;

/// The variable that names the bundle, when set.
pub const BUNDLE_VARIABLE: &str = "HAMLET_ENGINE_BUNDLE";
/// The bundle's folder beside the executable, as Hamlet is shipped.
pub const BUNDLE_FOLDER: &str = "plugins";
/// The bundle in a checkout, from the binary crate's folder.
pub const CHECKOUT_BUNDLE: &str = "../../plugins/dist";

/// Plugins that came with Hamlet and are now parts of another, by
/// their old id, each with the id of the plugin it is part of now.
/// Their panels kept their names there.
pub const MERGED: [(&str, &str); 7] = [
    ("hamlet.chat", "hamlet.agents"),
    ("hamlet.inspector", "hamlet.agents"),
    ("hamlet.skills", "hamlet.agents"),
    ("hamlet.petitions", "hamlet.agents"),
    ("hamlet.status", "hamlet.agents"),
    ("hamlet.settings", "hamlet.agents"),
    ("hamlet.welcome", "hamlet.agents"),
];

/// The plugin the bundled plugin of the old id `id` is part of now,
/// if it was merged into another.
pub fn merged_into(id: &str) -> Option<&'static str> {
    MERGED
        .iter()
        .find(|(old, _)| *old == id)
        .map(|(_, now)| *now)
}

/// The folder of the plugins that come with Hamlet.
#[derive(Resource, Clone, Debug)]
pub struct Bundle {
    root: PathBuf,
}

impl Bundle {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// [`BUNDLE_VARIABLE`] when it is set, else [`BUNDLE_FOLDER`]
    /// beside the executable when it is there, else
    /// [`CHECKOUT_BUNDLE`] from `crate_folder`, the binary crate's
    /// `CARGO_MANIFEST_DIR` as it was built.
    pub fn find(crate_folder: &str) -> Self {
        if let Some(root) = env::var_os(BUNDLE_VARIABLE)
            && !root.is_empty()
        {
            return Self::at(root);
        }
        let beside = env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join(BUNDLE_FOLDER)));
        if let Some(beside) = beside
            && beside.is_dir()
        {
            return Self::at(beside);
        }
        Self::at(Path::new(crate_folder).join(CHECKOUT_BUNDLE))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
