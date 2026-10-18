//! The plugins compiled into the app. Each is an optional
//! dependency behind a feature of its name.

use bevy_app::{PluginGroup, PluginGroupBuilder};

/// Every plugin whose feature is on. Add it to the app after
/// `hamlet_sdk::SdkPlugin`.
pub struct Builtin;

impl PluginGroup for Builtin {
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

    use super::Builtin;

    #[test]
    fn the_plugins_run_with_no_window() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SdkPlugin, Builtin));
        app.update();
        app.update();
    }
}
