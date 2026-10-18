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
    id: &'static str,
    wasm: Vec<u8>,
    /// The panel of a plugin that has no code.
    program: Option<&'static str>,
    asks: &'static [&'static str],
    grants: &'static [&'static str],
}

impl Plugin {
    pub fn new(id: &'static str, wasm: Vec<u8>) -> Self {
        Self {
            id,
            wasm,
            program: None,
            asks: &[],
            grants: &[],
        }
    }

    /// A plugin that is the panel `program` and no code.
    pub fn data(id: &'static str, program: &'static str) -> Self {
        Self {
            program: Some(program),
            ..Self::new(id, Vec::new())
        }
    }

    /// This, asking for these capabilities in its manifest.
    pub fn asking(mut self, asks: &'static [&'static str]) -> Self {
        self.asks = asks;
        self
    }

    /// This, approved with these grants.
    pub fn granted(
        mut self,
        grants: &'static [&'static str],
    ) -> Self {
        self.grants = grants;
        self
    }

    fn manifest(&self) -> String {
        let kind = match self.program {
            Some(_) => "kind = \"data\"",
            None => "interface = \"0.1.0\"",
        };
        let mut text = format!(
            "id = \"{0}\"\nname = \"{0}\"\nversion = \
             \"0.1.0\"\n{kind}\n",
            self.id
        );
        for ask in self.asks {
            text += &format!(
                "[[asks]]\ncapability = \"{ask}\"\nreason = \"a \
                 test\"\n"
            );
        }
        text
    }

    fn approval(&self) -> String {
        let grants = self
            .grants
            .iter()
            .map(|grant| format!("\"{grant}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{{ id = \"{}\", grants = [{grants}] }}", self.id)
    }
}

/// An app with `home` holding `plugins`, all approved.
pub fn app(home: &Scratch, plugins: &[Plugin]) -> App {
    let folder = home.0.join("plugins");
    for plugin in plugins {
        let dir = folder.join(plugin.id);
        std::fs::create_dir_all(&dir).expect("a plugin folder");
        match plugin.program {
            Some(program) => {
                std::fs::write(dir.join("panel.ui"), program)
                    .expect("a program");
            }
            None => {
                std::fs::write(dir.join("plugin.wasm"), &plugin.wasm)
                    .expect("a component");
            }
        }
        std::fs::write(dir.join("manifest.toml"), plugin.manifest())
            .expect("a manifest");
    }
    let ids = plugins
        .iter()
        .map(Plugin::approval)
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
        hamlet_sdk::agents::plugin,
        hamlet_sdk::dock::plugin,
        hamlet_sdk::plugin::plugin,
        hamlet_sdk::tree::plugin,
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
