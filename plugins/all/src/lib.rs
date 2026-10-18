//! The plugins compiled into the app. Each is an optional
//! dependency behind a feature of its name.

use bevy_app::{PluginGroup, PluginGroupBuilder};

/// Every plugin whose feature is on: the layers from the base up,
/// then the panels. Add it to the app after the SDK.
pub struct Builtin;

impl PluginGroup for Builtin {
    fn build(self) -> PluginGroupBuilder {
        let group =
            PluginGroupBuilder::start::<Self>().add_group(Base);
        // The realm draws into a window, so only an app with one
        // adds it.
        #[cfg(feature = "realm")]
        let group = group.add(plugin_realm::plugin);
        group.add_group(Panels)
    }
}

/// The layers that run with no window, the lowest first.
pub struct Base;

impl PluginGroup for Base {
    fn build(self) -> PluginGroupBuilder {
        let group = PluginGroupBuilder::start::<Self>();
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

    use super::Panels;

    #[test]
    fn the_panels_run_with_no_window() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SdkPlugin, Panels));
        app.update();
        app.update();
    }
}
