//! A scratch home with installed plugins, and an app that loads
//! them.

use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use bevy_fynix::FynixPlugin;
use hamlet_assets::Tuning;
use hamlet_base::HomeDir;
use hamlet_home::Home;
use hamlet_sdk::capability::{
    Capability, Choice, Choices, Record, Store,
};
use hamlet_sdk::theme::{HamletTheme, insert_theme};

use crate::interface::VERSION;
use crate::limits::Limits;

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

impl Scratch {
    /// The home.
    pub fn home(&self) -> PathBuf {
        self.0.clone()
    }

    /// The install folder of the home.
    pub fn plugins(&self) -> PathBuf {
        self.0.join("plugins")
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
    later: &'static [&'static str],
    /// The plugins it needs, each an id and a range.
    needs: &'static [(&'static str, &'static str)],
    /// Lines of its manifest, such as its exports and imports.
    lines: &'static str,
    /// The targets of scoped capabilities it is allowed, each a
    /// capability and a target.
    targets: &'static [(&'static str, &'static str)],
    /// What an ask is limited to, each a capability and its scope.
    scopes: &'static [(&'static str, &'static [&'static str])],
    /// Whether the chief installed it, as far as the store says.
    installed: bool,
}

impl Plugin {
    pub fn new(id: &'static str, wasm: Vec<u8>) -> Self {
        Self {
            id,
            wasm,
            program: None,
            asks: &[],
            grants: &[],
            later: &[],
            needs: &[],
            lines: "",
            targets: &[],
            scopes: &[],
            installed: true,
        }
    }

    /// This, with these lines added to its manifest.
    pub fn saying(mut self, lines: &'static str) -> Self {
        self.lines = lines;
        self
    }

    /// This, allowed these targets of scoped capabilities, each a
    /// capability and a target. It asks for the capabilities.
    pub fn allowed_for(
        mut self,
        targets: &'static [(&'static str, &'static str)],
    ) -> Self {
        self.targets = targets;
        self
    }

    /// This, with its asks limited to these scopes, each a capability
    /// that it asks for and what it is limited to.
    pub fn limited_to(
        mut self,
        scopes: &'static [(&'static str, &'static [&'static str])],
    ) -> Self {
        self.scopes = scopes;
        self
    }

    /// This, needing these plugins, each an id and a range.
    pub fn needing(
        mut self,
        needs: &'static [(&'static str, &'static str)],
    ) -> Self {
        self.needs = needs;
        self
    }

    /// This, in the install folder and not in the store: the chief is
    /// yet to be asked.
    pub fn uninstalled(mut self) -> Self {
        self.installed = false;
        self
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

    /// This, with the chief's choice for these set to ask next time.
    pub fn asking_next_time(
        mut self,
        later: &'static [&'static str],
    ) -> Self {
        self.later = later;
        self
    }

    fn manifest(&self) -> String {
        let kind = match self.program {
            Some(_) => "kind = \"data\"".to_string(),
            None => format!("interface = \"{VERSION}\""),
        };
        let mut text = format!(
            "id = \"{0}\"\nname = \"{0}\"\nversion = \
             \"0.1.0\"\n{kind}\n",
            self.id
        );
        for (id, range) in self.needs {
            text += &format!(
                "[[needs]]\nid = \"{id}\"\nrange = \"{range}\"\n"
            );
        }
        text += self.lines;
        let scoped =
            self.targets.iter().map(|(capability, _)| capability);
        for ask in self.asks.iter().chain(scoped) {
            text += &format!(
                "[[asks]]\ncapability = \"{ask}\"\nreason = \"a \
                 test\"\n"
            );
            if let Some((_, scope)) =
                self.scopes.iter().find(|(name, _)| name == ask)
            {
                text += &format!("scope = {scope:?}\n");
            }
        }
        text
    }

    /// The choices of a chief who allowed `grants` and set `later` to
    /// ask next time, among what the plugin asks for.
    fn choices(&self) -> Choices {
        let of = |names: &'static [&'static str], choice| {
            names
                .iter()
                .filter(|name| self.asks.contains(name))
                .filter_map(|name| Capability::parse(name).ok())
                .map(move |capability| (capability, choice))
        };
        let choices = of(self.grants, Choice::Allow)
            .chain(of(self.later, Choice::AskNextTime))
            .fold(
                Choices::default(),
                |choices, (capability, choice)| {
                    choices.decided(capability, choice, false)
                },
            );
        self.targets
            .iter()
            .filter_map(|(name, target)| {
                Some((Capability::parse(name).ok()?, *target))
            })
            .fold(choices, |choices, (capability, target)| {
                choices
                    .decided(capability, Choice::AskNextTime, false)
                    .decided_for(
                        capability,
                        target,
                        Choice::Allow,
                        false,
                    )
            })
    }
}

/// An app with `home` holding `plugins`, all installed as if the
/// chief had allowed what each is granted.
pub fn app(home: &Scratch, plugins: &[Plugin]) -> App {
    put(home, plugins);
    open(home)
}

/// Puts `plugins` in the home, and in its store those that are
/// installed.
pub fn put(home: &Scratch, plugins: &[Plugin]) {
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
    let mut store = Store::open(&folder);
    for plugin in plugins.iter().filter(|plugin| plugin.installed) {
        // As the chief's answer keeps the scope it was asked with.
        let record = Record::installed("0.1.0", &plugin.choices());
        let record = match crate::install::read(
            &folder.join(plugin.id),
            &Limits::default(),
        ) {
            Ok(read) => record.scoped(&read.asks),
            Err(_) => record,
        };
        store.set(plugin.id, record);
    }
}

/// An app that loads the home as it is.
pub fn open(home: &Scratch) -> App {
    let mut app = App::new();
    app.insert_resource(HomeDir(Home {
        root: home.0.clone(),
        workspace: home.0.join("workspace"),
    }))
    .add_plugins((
        hamlet_agents_api::agents::plugin,
        hamlet_agents_api::petition::plugin,
        hamlet_sdk::capability::plugin,
        hamlet_sdk::command::plugin,
        hamlet_sdk::context::plugin,
        hamlet_sdk::dock::plugin,
        hamlet_sdk::inspector::plugin,
        hamlet_sdk::keys::plugin,
        hamlet_sdk::menu::plugin,
        hamlet_sdk::plugin::plugin,
        hamlet_sdk::settings::plugin,
        hamlet_sdk::tree::plugin,
        crate::plugin,
    ));
    // Tests run side by side, and a plugin must not fail for being
    // slow on a busy machine.
    app.insert_resource(Limits::default().patient());
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
