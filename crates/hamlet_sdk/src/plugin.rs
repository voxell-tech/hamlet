//! The plugin frame: what a plugin is, how it is added to the app
//! and how it is switched on and off while the app runs.

mod add;
mod contribution;
mod enable;
mod entry;
mod info;
mod manifest;
mod status;

use bevy::app::App;

pub use self::add::AppPluginExt;
pub use self::contribution::{ContributedBy, Contributions};
pub use self::enable::{Disable, Enable};
pub use self::entry::{Entry, EntryPoint};
pub use self::info::PluginInfo;
pub use self::manifest::{Manifest, Need};
pub use self::status::Status;

/// Switches plugins on and off by their id.
pub fn plugin(app: &mut App) {
    app.add_observer(enable::enable)
        .add_observer(enable::disable);
}

/// A unit of function the app is made of. Built-in plugins are
/// added with [`AppPluginExt::add_hamlet_plugin`].
pub trait HamletPlugin {
    fn manifest() -> Manifest;

    /// Declares what the plugin adds. It runs each time the plugin is
    /// enabled.
    fn entry(entry: &mut Entry);
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy_fynix::ViewExt;
    use bevy_fynix::dock::DockRegistry;
    use bevy_fynix::views::label;

    use super::*;
    use crate::dock;
    use crate::icons::Glyph;
    use crate::theme::HamletTheme;

    struct Probe;

    impl HamletPlugin for Probe {
        fn manifest() -> Manifest {
            Manifest::built_in("probe")
        }

        fn entry(entry: &mut Entry) {
            entry.panel("probe", "PROBE", Glyph::Book, || {
                label("x").boxed()
            });
        }
    }

    fn listed(app: &App) -> bool {
        app.world()
            .resource::<DockRegistry<HamletTheme>>()
            .get("probe")
            .is_some()
    }

    #[test]
    fn a_panel_lives_as_long_as_its_plugin_is_enabled() {
        let mut app = App::new();
        app.add_plugins((dock::plugin, plugin))
            .add_hamlet_plugin::<Probe>();
        assert!(listed(&app));

        app.world_mut().trigger(Disable("probe".into()));
        app.world_mut().flush();
        assert!(!listed(&app));

        app.world_mut().trigger(Enable("probe".into()));
        app.world_mut().flush();
        assert!(listed(&app));
    }
}
