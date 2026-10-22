//! The desktop service: the machine the engine runs on. It keeps the
//! machine awake while an agent works, as the person set it, opens an
//! `https` address or a file for a plugin holding `desktop.open` on
//! it, and tells the plugins that watch it of the address under the
//! pointer. A later launch of the engine on the same home shows the
//! running engine's window.

mod answer;
mod awake;
mod launch;
mod open;

#[cfg(all(test, unix))]
mod tests;

use bevy::app::App;

pub use crate::awake::KeepAwake;
pub use crate::launch::{Launches, claim};
pub use crate::open::{Opening, opening};

pub fn plugin(app: &mut App) {
    app.add_plugins((
        awake::plugin,
        open::plugin,
        answer::plugin,
        launch::plugin,
    ));
}
