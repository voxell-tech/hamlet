//! The menu of a thing in the world and the items plugins add to it.

mod item;
mod model;

use bevy::app::{App, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;

pub use self::item::{ContextItem, ContextWhen};
pub use self::model::{ContextItems, Line};

/// Keeps [`ContextItems`] in step with the [`ContextItem`] entities
/// there are.
pub fn plugin(app: &mut App) {
    app.init_resource::<ContextItems>()
        .add_systems(Update, model::keep.run_if(model::moved));
}

#[cfg(test)]
mod tests {
    use bevy::app::App;

    use super::*;
    use crate::capability::{
        Asked, Asks, Capability, Choice, Choices, Granted, Scope,
    };
    use crate::command::{self, Command, Subject, kind};
    use crate::plugin::{
        self as plugins, AppPluginExt, ContributedBy, Disable, Entry,
        HamletPlugin, Manifest, PluginInfo,
    };

    struct Probe;

    impl HamletPlugin for Probe {
        fn manifest() -> Manifest {
            Manifest::built_in("probe")
        }

        fn entry(entry: &mut Entry) {
            entry.command_with("greet", "Greet", |_, _| {});
            entry.context_item(
                kind::AGENT,
                0,
                "Greet {name}",
                "greet",
                None,
            );
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((plugins::plugin, command::plugin, plugin));
        app
    }

    fn labels(app: &App, kind: &str) -> Vec<String> {
        let who = Subject::new(kind, "1", "Juniper");
        let items = app.world().resource::<ContextItems>();
        items.of(kind).map(|line| line.said(&who)).collect()
    }

    #[test]
    fn an_item_names_its_thing_and_goes_with_its_plugin() {
        let mut app = app();
        app.add_hamlet_plugin::<Probe>();
        app.update();
        assert_eq!(labels(&app, kind::AGENT), ["Greet Juniper"]);
        assert!(labels(&app, kind::HOUSE).is_empty());

        app.world_mut().trigger(Disable("probe".into()));
        app.world_mut().flush();
        app.update();
        assert!(labels(&app, kind::AGENT).is_empty());
    }

    #[test]
    fn an_installed_plugin_shows_an_item_only_while_ui_context_is_allowed()
     {
        let mut app = app();
        let world = app.world_mut();
        let plugin = world
            .spawn(PluginInfo {
                id: "far".into(),
                name: "Far".into(),
                version: "1".into(),
                builtin: false,
                essential: false,
            })
            .id();
        world
            .spawn((Command::new("go", "Go"), ContributedBy(plugin)));
        world.spawn((
            ContextItem {
                target: kind::AGENT.into(),
                section: 0,
                label: "Go".into(),
                command: "go".into(),
                icon: None,
            },
            ContributedBy(plugin),
        ));
        let verdict = |choice| {
            let asks = Asks(vec![Asked {
                capability: Capability::UiContext,
                reason: "a test".into(),
                scope: Scope::default(),
            }]);
            let choices = Choices::default().decided(
                Capability::UiContext,
                choice,
                false,
            );
            Granted::of(&asks, &choices)
        };

        app.update();
        assert!(labels(&app, kind::AGENT).is_empty());
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
            assert_eq!(
                labels(&app, kind::AGENT).len(),
                shown,
                "{choice:?}"
            );
        }
    }
}
