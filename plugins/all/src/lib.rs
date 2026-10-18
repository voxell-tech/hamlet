//! The plugins compiled into the app. Each is an optional
//! dependency behind a feature of its name.

use bevy_app::{PluginGroup, PluginGroupBuilder};

/// Every plugin whose feature is on: the layers, then the panels.
/// Add it to the app after the SDK.
pub struct Builtin;

impl PluginGroup for Builtin {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add_group(Layers)
            .add_group(Panels)
    }
}

/// The layers a window is drawn from. The realm draws into a window,
/// so it is left out of an app with none.
pub struct Layers;

impl PluginGroup for Layers {
    fn build(self) -> PluginGroupBuilder {
        let group = PluginGroupBuilder::start::<Self>();
        #[cfg(feature = "realm")]
        let group = group.add(plugin_realm::plugin);
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

    use super::Panels;

    #[test]
    fn the_panels_run_with_no_window() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SdkPlugin, Panels));
        app.update();
        app.update();
    }
}
