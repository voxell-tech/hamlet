//! The plugins compiled into the app. Each is an optional
//! dependency behind a feature of its name.

use bevy_app::{PluginGroup, PluginGroupBuilder};
#[cfg(feature = "agents")]
pub use plugin_agents::Launch;

/// Every plugin whose feature is on: the layers from the base up,
/// the window's frame, the realm, what fills the frame and then the
/// panels. Add it to the app after Bevy's default plugins and a
/// `ShowRequests` resource.
pub struct Builtin;

impl PluginGroup for Builtin {
    fn build(self) -> PluginGroupBuilder {
        let group =
            PluginGroupBuilder::start::<Self>().add_group(Base);
        // The window reads the agents when it is built.
        #[cfg(feature = "window")]
        let group = group.add(hamlet_ui::UiPlugin);
        // The realm draws into a window, so only an app with one
        // adds it.
        #[cfg(feature = "realm")]
        let group = group.add(plugin_realm::plugin);
        group.add_group(Frame).add_group(Panels)
    }
}

/// The plugins that fill the window's frame: the world's panel, the
/// menus, the status bar and the first-run screen.
pub struct Frame;

impl PluginGroup for Frame {
    fn build(self) -> PluginGroupBuilder {
        let group = PluginGroupBuilder::start::<Self>();
        #[cfg(feature = "world-view")]
        let group = group.add(plugin_world_view::plugin);
        #[cfg(feature = "menus")]
        let group = group.add(plugin_menus::plugin);
        #[cfg(feature = "status")]
        let group = group.add(plugin_status::plugin);
        #[cfg(feature = "welcome")]
        let group = group.add(plugin_welcome::plugin);
        group
    }
}

/// The layers that run with no window, the lowest first.
pub struct Base;

impl PluginGroup for Base {
    fn build(self) -> PluginGroupBuilder {
        let group = PluginGroupBuilder::start::<Self>();
        #[cfg(feature = "agents")]
        let group = group.add(plugin_agents::plugin);
        #[cfg(feature = "hamlets")]
        let group = group.add(plugin_hamlets::plugin);
        group
    }
}

/// The plugins that add a panel.
pub struct Panels;

impl PluginGroup for Panels {
    fn build(self) -> PluginGroupBuilder {
        let group = PluginGroupBuilder::start::<Self>();
        #[cfg(feature = "inspector")]
        let group = group.add(plugin_inspector::plugin);
        #[cfg(feature = "needs")]
        let group = group.add(plugin_needs::plugin);
        #[cfg(feature = "settings")]
        let group = group.add(plugin_settings::plugin);
        #[cfg(feature = "skills")]
        let group = group.add(plugin_skills::plugin);
        group
    }
}

#[cfg(test)]
mod tests {
    use bevy::MinimalPlugins;
    use bevy_app::App;
    use hamlet_sdk::SdkPlugin;
    use hamlet_sdk::plugin::{HamletPlugin, Manifest, RESERVED};

    use super::Panels;

    #[test]
    fn every_built_in_id_is_reserved() {
        let manifests: [Manifest; _] = [
            #[cfg(feature = "agents")]
            plugin_agents::AgentsPlugin::manifest(),
            #[cfg(feature = "hamlets")]
            plugin_hamlets::HamletsPlugin::manifest(),
            #[cfg(feature = "inspector")]
            plugin_inspector::InspectorPlugin::manifest(),
            #[cfg(feature = "menus")]
            plugin_menus::MenusPlugin::manifest(),
            #[cfg(feature = "needs")]
            plugin_needs::NeedsPlugin::manifest(),
            #[cfg(feature = "realm")]
            plugin_realm::RealmLayer::manifest(),
            #[cfg(feature = "settings")]
            plugin_settings::SettingsPlugin::manifest(),
            #[cfg(feature = "skills")]
            plugin_skills::SkillsPlugin::manifest(),
            #[cfg(feature = "status")]
            plugin_status::StatusPlugin::manifest(),
            #[cfg(feature = "welcome")]
            plugin_welcome::WelcomePlugin::manifest(),
            #[cfg(feature = "window")]
            hamlet_ui::commands::Window::manifest(),
            #[cfg(feature = "world-view")]
            plugin_world_view::WorldViewPlugin::manifest(),
        ];
        for manifest in manifests {
            assert!(
                RESERVED.contains(&manifest.id.as_str()),
                "{}",
                manifest.id
            );
        }
    }

    #[test]
    fn the_panels_run_with_no_window() {
        use hamlet_agents_api::{agents, convo, papers, roll, waits};

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            SdkPlugin,
            (
                convo::plugin,
                agents::plugin,
                papers::plugin,
                roll::plugin,
                waits::plugin,
            ),
            Panels,
        ));
        app.update();
        app.update();
    }
}
