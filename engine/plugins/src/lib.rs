//! Plugins as entities. A plugin is an entity with a [`PluginInfo`].
//! What it adds are entities linked to it by [`ContributedBy`], so
//! switching it off despawns them.

mod entry;
mod list;
mod switch;

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use hamlet_engine_api::{Manifest, PluginId};
use hamlet_engine_core::LogLine;

pub use self::entry::{Entry, EntryPoint};
pub use self::switch::{
    ContributedBy, Contributions, Disable, Enable, Enabled,
};

pub fn plugin(app: &mut App) {
    app.add_plugins((switch::plugin, list::plugin));
}

/// On a plugin's entity: who it is, from its manifest.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct PluginInfo {
    pub id: PluginId,
    pub name: String,
    pub version: String,
    /// Built into the engine, rather than installed.
    pub builtin: bool,
}

/// Adds the plugin `manifest` describes, built into the engine with
/// its `entry`, and switches it on. A manifest the engine cannot take
/// is told in the log, and nothing is added.
pub fn add_builtin(
    commands: &mut Commands,
    manifest: Manifest,
    entry: fn(&mut Entry),
) -> Option<Entity> {
    if let Err(why) = manifest.check() {
        let line = LogLine::error(
            "plugins",
            format!("{} is not added: {why}", manifest.id),
        );
        commands.queue(move |world: &mut World| {
            world.write_message(line);
        });
        return None;
    }
    let plugin = commands
        .spawn((
            PluginInfo {
                id: manifest.id,
                name: manifest.name,
                version: manifest.version,
                builtin: true,
            },
            EntryPoint(entry),
        ))
        .id();
    commands.trigger(Enable { entity: plugin });
    Some(plugin)
}

#[cfg(test)]
mod tests {
    use bevy::ecs::query::With;
    use bevy::time::TimePlugin;
    use bevy_fynix::ViewExt;
    use bevy_fynix::views::label;
    use hamlet_engine_api::{API_VERSION, Layout};
    use hamlet_engine_core::Home;
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
            .command("write", "Write", |_| {})
            .menu_entry("Notes", "write")
            .shortcut("Primary+Shift+N", "write")
            .settings_page("looks", "Looks", content)
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
        let manifest = Manifest {
            id: PluginId::try_from("hamlet.notes").unwrap(),
            name: "Notes".into(),
            version: "0.1.0".into(),
            api: API_VERSION.into(),
            asks: Vec::new(),
        };
        let mut commands = app.world_mut().commands();
        let notes =
            add_builtin(&mut commands, manifest, entry).unwrap();
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
        // The plugin list is the engine's own panel.
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
