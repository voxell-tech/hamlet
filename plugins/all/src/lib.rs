//! The plugins compiled into the app. Each is an optional
//! dependency behind a feature of its name.

use bevy_app::{PluginGroup, PluginGroupBuilder};

/// Every plugin whose feature is on. Add it to the app after
/// `hamlet_sdk::SdkPlugin`.
pub struct Builtin;

impl PluginGroup for Builtin {
    fn build(self) -> PluginGroupBuilder {
        let group = PluginGroupBuilder::start::<Self>();
        #[cfg(feature = "needs")]
        let group = group.add(plugin_needs::plugin);
        #[cfg(feature = "skills")]
        let group = group.add(plugin_skills::plugin);
        group
    }
}
