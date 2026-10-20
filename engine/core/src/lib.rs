//! What every engine module stands on: the tuning files under
//! `engine/assets/`, the home folder, the engine's log and the
//! budgets a plugin spends.

pub mod budget;
pub mod home;
pub mod log;
pub mod tuning;

use bevy::app::App;
pub use budget::Budgets;
pub use home::Home;
pub use log::{Log, LogLevel, LogLine};
pub use tuning::{Tuning, TuningFile, TuningPlugin};

pub fn plugin(app: &mut App) {
    app.add_plugins((
        home::plugin,
        log::plugin,
        TuningPlugin::<Budgets>::default(),
    ));
    #[cfg(feature = "hot-reload")]
    app.add_plugins(tuning::plugin);
}
