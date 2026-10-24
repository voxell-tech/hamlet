//! The scene the world is drawn in, with no hamlets.

use bevy::prelude::*;

use crate::{
    debug, keeper, palette, play, settings, sky, sound, stage, stock,
};

/// The scene: tuning, colours, camera, clock, robots and the shared
/// materials. Add it after Bevy's default plugins.
pub fn plugin(app: &mut App) {
    // In this order: each reads what those before it put in.
    app.add_plugins((
        settings::plugin,
        palette::plugin,
        play::plugin,
        sound::plugin,
        debug::plugin,
        sky::plugin,
        stage::plugin,
        keeper::plugin,
        stock::plugin,
    ));
}
