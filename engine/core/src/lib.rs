//! What every engine module stands on: the tuning files under
//! `engine/tuning/`, the home folder, the bundle of plugins that come
//! with Hamlet, the engine's log, the guards and limits plugins are
//! held to and writing a file whole.

pub mod budget;
pub mod bundle;
pub mod file;
pub mod home;
pub mod log;
pub mod tuning;

use bevy::app::App;

pub use crate::budget::{ActiveLimits, Guards, StartingLimits};
pub use crate::bundle::{Bundle, merged_into};
pub use crate::file::write_whole;
pub use crate::home::Home;
pub use crate::log::{Log, LogLevel, LogLine};
pub use crate::tuning::{Tuning, TuningFile, TuningPlugin};

pub fn plugin(app: &mut App) {
    app.add_plugins((
        home::plugin,
        log::plugin,
        TuningPlugin::<Guards>::default(),
        TuningPlugin::<StartingLimits>::default(),
    ));
    // A release build is run from anywhere, so where its files were
    // built from is not watched unless it is asked for.
    #[cfg(any(feature = "hot-reload", debug_assertions))]
    app.add_plugins(tuning::plugin);
}
