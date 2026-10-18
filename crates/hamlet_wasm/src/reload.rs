//! Following the files of an installed plugin. When one changes, the
//! new version is built beside the running one and takes its place
//! only if it starts.

pub mod keep;
mod polling;
mod swap;
mod wait;
mod watch;

use bevy::prelude::*;

pub use self::polling::Polling;
pub use self::wait::Waiting;
pub use self::watch::{Modified, Stamp, Watch};
use crate::PluginSet;

/// Looks at the files of the installed plugins and reloads a plugin
/// whose files changed.
pub fn plugin(app: &mut App) {
    app.init_resource::<Polling>()
        .add_message::<Modified>()
        .add_observer(keep::rollback)
        .add_systems(
            Update,
            (watch::poll, wait::answered, swap::reload)
                .chain()
                .before(PluginSet::Collect),
        );
}
