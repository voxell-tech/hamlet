//! The menus of the top bar and the lines plugins add to them.

mod entry;
mod model;

use bevy::app::{App, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;

pub use self::entry::{Menu, MenuEntry};
pub use self::model::{Line, Menus};

/// Keeps [`Menus`] in step with the [`MenuEntry`] entities there are.
pub fn plugin(app: &mut App) {
    app.init_resource::<Menus>()
        .add_systems(Update, model::keep.run_if(model::moved));
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::resource::Resource;

    use super::*;
    use crate::capability::{
        Asked, Asks, Capability, Choice, Choices, Granted, Scope,
    };
    use crate::command::{self, Command, RunCommand};
    use crate::plugin::{
        self as plugins, AppPluginExt, ContributedBy, Disable, Entry,
        HamletPlugin, Manifest, PluginInfo,
    };

    #[derive(Resource)]
    struct Waved;

    struct Probe;

    impl HamletPlugin for Probe {
        fn manifest() -> Manifest {
            Manifest::built_in("probe")
        }

        fn entry(entry: &mut Entry) {
            entry.command("wave", "Wave", |commands| {
                commands.insert_resource(Waved);
            });
            entry.menu_item(Menu::View, 0, "Wave", "wave", None);
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((plugins::plugin, command::plugin, plugin));
        app
    }

    fn labels(app: &App) -> Vec<&str> {
        let menus = app.world().resource::<Menus>();
        menus.0.iter().map(|line| line.label.as_str()).collect()
    }

    #[test]
    fn an_entry_runs_its_command_and_goes_with_its_plugin() {
        let mut app = app();
        app.add_hamlet_plugin::<Probe>();
        app.update();
        assert_eq!(labels(&app), ["Wave"]);

        let id = app.world().resource::<Menus>().0[0].command.clone();
        app.world_mut().trigger(RunCommand::of(id));
        app.world_mut().flush();
        assert!(app.world().contains_resource::<Waved>());

        app.world_mut().trigger(Disable("probe".into()));
        app.world_mut().flush();
        app.update();
        assert!(labels(&app).is_empty());
    }

    #[test]
    fn an_installed_plugin_shows_a_line_only_while_ui_menu_is_allowed()
     {
        let mut app = app();
        let world = app.world_mut();
        let plugin = world
            .spawn(PluginInfo {
                id: "far".into(),
                name: "Far".into(),
                version: "1".into(),
                builtin: false,
            })
            .id();
        world
            .spawn((Command::new("go", "Go"), ContributedBy(plugin)));
        world.spawn((
            MenuEntry {
                menu: Menu::App,
                section: 0,
                label: "Go".into(),
                command: "go".into(),
                icon: None,
            },
            ContributedBy(plugin),
        ));
        let verdict = |choice| {
            let asks = Asks(vec![Asked {
                capability: Capability::UiMenu,
                reason: "a test".into(),
                scope: Scope::default(),
            }]);
            let choices = Choices::default().decided(
                Capability::UiMenu,
                choice,
                false,
            );
            Granted::of(&asks, &choices)
        };

        app.update();
        assert!(labels(&app).is_empty());
        for (choice, shown) in [
            (Choice::Allow, 1),
            (Choice::Deny, 0),
            (Choice::AskNextTime, 0),
            (Choice::Allow, 1),
        ] {
            app.world_mut()
                .entity_mut(plugin)
                .insert(verdict(choice));
            app.update();
            assert_eq!(labels(&app).len(), shown, "{choice:?}");
        }
    }
}
