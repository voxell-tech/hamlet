//! Plugins as entities. A plugin is an entity with a [`PluginInfo`].
//! What it adds are entities linked to it by [`ContributedBy`], so
//! switching it off despawns them.

mod entry;
mod env;
mod limits;
mod list;
mod mail;
mod needs;
mod switch;
mod switched_off;

use bevy::app::App;
use bevy::ecs::component::Component;
use hamlet_engine_api::{
    Ask, AssetTypes, Capability, Dependency, Events, Limits,
    Manifest, PluginId, Services,
};

pub use crate::entry::{Entry, EntryPoint};
pub use crate::env::default_port;
pub use crate::limits::ChosenLimits;
pub use crate::list::{AddPlugin, PluginPageSection, Uninstall};
pub use crate::mail::{Asked, Deliver};
pub use crate::needs::{Blocked, StartOrder, Starting, resolve};
pub use crate::switch::{
    ContributedBy, Contributions, Disable, Enable, Enabled,
    PluginCommandsExt, Stopped,
};
pub use crate::switched_off::SwitchedOff;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        switched_off::plugin,
        limits::plugin,
        switch::plugin,
        needs::plugin,
        list::plugin,
        mail::plugin,
    ));
}

/// On a plugin's entity: who it is, from its manifest.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct PluginInfo {
    pub id: PluginId,
    pub name: String,
    pub version: String,
    pub kind: PluginKind,
    /// What it asks the person for, and why, each `net.fetch` on a
    /// variable on the host the variable's address is on.
    pub asks: Vec<Ask>,
    /// The targets of its `net.fetch` asks on a variable, each with
    /// the variable's name.
    pub variables: Vec<(String, String)>,
    /// Its environment: each variable its manifest names, with the
    /// engine's value when that is set and not empty, else the
    /// manifest's.
    pub env: Vec<(String, String)>,
    pub assets: AssetTypes,
    pub services: Services,
    pub events: Events,
    /// The limits of its instance it asks for in place of the
    /// engine's.
    pub limits: Limits,
    /// The plugins it needs, by id.
    pub dependencies: Vec<Dependency>,
}

/// Where a plugin comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PluginKind {
    /// Rust, linked into the binary.
    BuiltIn,
    /// WASM that comes with Hamlet, read from the bundle folder.
    Bundled,
    /// WASM the person installed in the home.
    Installed,
}

impl PluginInfo {
    /// The plugin `manifest` describes, in the engine's environment.
    pub fn of(manifest: Manifest, kind: PluginKind) -> Self {
        Self::looking_up(manifest, kind, |name| {
            std::env::var(name).ok()
        })
    }

    /// The plugin `manifest` describes, with the value of each
    /// variable of the environment as `lookup` gives it.
    pub fn looking_up(
        manifest: Manifest,
        kind: PluginKind,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Self {
        let env = manifest.environment(lookup);
        let (asks, variables) = env::resolve(&manifest, &env);
        Self {
            dependencies: manifest.needs(),
            id: manifest.id,
            name: manifest.name,
            version: manifest.version,
            kind,
            asks,
            variables,
            env,
            assets: manifest.assets,
            services: manifest.services,
            events: manifest.events,
            limits: manifest.limits,
        }
    }

    /// `target` as the person is shown it for `capability`: with the
    /// name of the variable it was read from, as
    /// `localhost:8787 (HAMLET_BAZAAR_URL)`.
    pub fn shown(
        &self,
        capability: Capability,
        target: &str,
    ) -> String {
        let named = self
            .variables
            .iter()
            .find(|(resolved, _)| resolved == target)
            .filter(|_| capability == Capability::NetFetch);
        match named {
            Some((_, name)) => format!("{target} ({name})"),
            None => target.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::entity::Entity;
    use bevy::ecs::query::With;
    use bevy::time::TimePlugin;
    use bevy_fynix::ViewExt;
    use bevy_fynix::views::label;
    use hamlet_engine_api::Layout;
    use hamlet_engine_core::Home;
    use hamlet_engine_shell::icons::Glyph;
    use hamlet_engine_shell::{
        AnyView, Command, MenuEntry, Panel, SettingsPage, Shortcut,
        SuggestedWorkspace,
    };

    use super::*;

    fn content(_: Entity) -> AnyView {
        label("").boxed()
    }

    fn entry(entry: &mut Entry) {
        entry
            .panel("notes", "Notes", content)
            .command("write", "Write", |_, _| {})
            .menu_entry("Notes", "write")
            .shortcut("Primary+Shift+N", "write")
            .settings_page(
                "style",
                "Style",
                "",
                Glyph::Sliders,
                content,
            )
            .workspace("notes", "Notes", Layout::empty());
    }

    #[test]
    fn switching_a_plugin_off_takes_away_all_it_added() {
        let root = std::env::temp_dir().join(format!(
            "hamlet_engine_test_plugins_{}",
            std::process::id()
        ));
        let mut app = App::new();
        app.insert_resource(Home::at(&root)).add_plugins((
            TimePlugin,
            hamlet_engine_core::plugin,
            plugin,
        ));
        let manifest = Manifest::new(
            PluginId::try_from("hamlet.notes").unwrap(),
            "Notes",
            "0.1.0",
        );
        let notes = app
            .world_mut()
            .commands()
            .add_builtin(manifest, entry)
            .unwrap();
        app.update();

        let count = |app: &mut App| {
            let world = app.world_mut();
            [
                world
                    .query_filtered::<(), With<Panel>>()
                    .iter(world)
                    .count(),
                world
                    .query_filtered::<(), With<Command>>()
                    .iter(world)
                    .count(),
                world
                    .query_filtered::<(), With<MenuEntry>>()
                    .iter(world)
                    .count(),
                world
                    .query_filtered::<(), With<Shortcut>>()
                    .iter(world)
                    .count(),
                world
                    .query_filtered::<(), With<SettingsPage>>()
                    .iter(world)
                    .count(),
                world
                    .query_filtered::<(), With<SuggestedWorkspace>>()
                    .iter(world)
                    .count(),
            ]
        };
        // The Plugins panel is the engine's own.
        assert_eq!(count(&mut app), [2, 1, 1, 1, 1, 1]);

        app.world_mut().trigger(Disable { entity: notes });
        app.update();
        assert_eq!(count(&mut app), [1, 0, 0, 0, 0, 0]);
        assert!(!app.world().entity(notes).contains::<Enabled>());

        app.world_mut().trigger(Enable { entity: notes });
        app.update();
        assert_eq!(count(&mut app), [2, 1, 1, 1, 1, 1]);
        let _ = std::fs::remove_dir_all(root);
    }
}
