//! A scratch home with installed plugins, and an app that loads
//! them.

use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use bevy_fynix::FynixPlugin;
use hamlet_assets::Tuning;
use hamlet_core::core::HomeDir;
use hamlet_home::Home;
use hamlet_sdk::theme::{HamletTheme, insert_theme};

/// A folder that is removed when it goes.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "hamlet_wasm_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        Self(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A plugin to install and approve.
pub struct Plugin {
    pub id: &'static str,
    pub wasm: Vec<u8>,
}

/// An app with `home` holding `plugins`, all approved.
pub fn app(home: &Scratch, plugins: &[Plugin]) -> App {
    let folder = home.0.join("plugins");
    for plugin in plugins {
        let dir = folder.join(plugin.id);
        std::fs::create_dir_all(&dir).expect("a plugin folder");
        std::fs::write(dir.join("plugin.wasm"), &plugin.wasm)
            .expect("a component");
        std::fs::write(
            dir.join("manifest.toml"),
            format!(
                "id = \"{0}\"\nname = \"{0}\"\nversion = \
                 \"0.1.0\"\ninterface = \"0.1.0\"\n",
                plugin.id
            ),
        )
        .expect("a manifest");
    }
    let ids = plugins
        .iter()
        .map(|plugin| format!("\"{}\"", plugin.id))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        folder.join("approved.toml"),
        format!("plugins = [{ids}]\n"),
    )
    .expect("an approved list");
    let mut app = App::new();
    app.insert_resource(HomeDir(Home {
        root: home.0.clone(),
        workspace: home.0.join("workspace"),
    }))
    .add_plugins((
        hamlet_sdk::dock::plugin,
        hamlet_sdk::plugin::plugin,
        crate::plugin,
    ));
    app
}

/// What the panels' views need to be built in `app`.
pub fn views(app: &mut App) {
    app.add_plugins((
        TimePlugin,
        FynixPlugin::<HamletTheme>::default(),
    ));
    insert_theme(app.world_mut(), HamletTheme::designed());
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        Duration::from_millis(50),
    ));
}
