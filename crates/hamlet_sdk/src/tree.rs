//! The tree of a plugin's panel: the model, what is drawn, the view
//! built from it and what the user does in it.

pub mod event;
pub mod glyph;
pub mod id;
pub mod node;
pub mod store;
pub mod view;

use bevy::app::App;

use self::store::{PluginTrees, forget};

/// Keeps what each plugin panel shows, and drops it when the panel
/// goes.
pub fn plugin(app: &mut App) {
    app.init_resource::<PluginTrees>().add_observer(forget);
}
