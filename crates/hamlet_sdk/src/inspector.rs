//! The tabs plugins add to the inspector's page of an agent.

mod model;
mod section;

use bevy::app::{App, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;

pub use self::model::{InspectorSections, Line};
pub use self::section::InspectorSection;

/// The kinds of thing whose inspector page has tabs a plugin adds.
pub const KINDS: [&str; 1] = [crate::command::kind::AGENT];

/// Keeps [`InspectorSections`] in step with the [`InspectorSection`]
/// entities there are.
pub fn plugin(app: &mut App) {
    app.init_resource::<InspectorSections>()
        .add_systems(Update, model::keep.run_if(model::moved));
}

#[cfg(test)]
mod tests {
    use bevy::app::App;

    use super::*;
    use crate::AnyView;
    use crate::capability::{
        Asked, Asks, Capability, Choice, Choices, Granted, Scope,
    };
    use crate::command::kind;
    use crate::icons::Glyph;
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
            entry.panel("probe/tab", "TAB", Glyph::Book, || {
                AnyView::new(|cx| {
                    cx.build(bevy_fynix::views::frame())
                })
            });
            entry.inspector_section(kind::AGENT, "TAB", "probe/tab");
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((plugins::plugin, plugin));
        app
    }

    fn titles(app: &App) -> Vec<String> {
        let sections = app.world().resource::<InspectorSections>();
        sections.of(kind::AGENT).map(|l| l.title.clone()).collect()
    }

    #[test]
    fn a_tab_takes_the_icon_of_its_panel_and_goes_with_its_plugin() {
        let mut app = app();
        app.add_hamlet_plugin::<Probe>();
        app.update();
        assert_eq!(titles(&app), ["TAB"]);
        let icon =
            app.world().resource::<InspectorSections>().0[0].icon;
        assert_eq!(icon, Glyph::Book);

        app.world_mut().trigger(Disable("probe".into()));
        app.world_mut().flush();
        app.update();
        assert!(titles(&app).is_empty());
    }

    #[test]
    fn an_installed_plugin_shows_a_tab_only_while_ui_inspector_is_allowed()
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
        world.spawn((
            InspectorSection {
                target: kind::AGENT.into(),
                title: "FAR".into(),
                panel: "far/main".into(),
            },
            ContributedBy(plugin),
        ));
        let verdict = |choice| {
            let asks = Asks(vec![Asked {
                capability: Capability::UiInspector,
                reason: "a test".into(),
                scope: Scope::default(),
            }]);
            let choices = Choices::default().decided(
                Capability::UiInspector,
                choice,
                false,
            );
            Granted::of(&asks, &choices)
        };

        app.update();
        assert!(titles(&app).is_empty());
        for (choice, shown) in [
            (Choice::Allow, 1),
            (Choice::AskNextTime, 0),
            (Choice::Allow, 1),
        ] {
            app.world_mut()
                .entity_mut(plugin)
                .insert(verdict(choice));
            app.update();
            assert_eq!(titles(&app).len(), shown, "{choice:?}");
        }
    }
}
