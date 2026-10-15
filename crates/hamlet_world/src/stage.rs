//! The scene's clock, and the stage its owner sets.

use bevy::prelude::*;

/// Scene time. Stills freeze it so that every run renders the same
/// frame.
#[derive(Resource, Default)]
pub struct Clock {
    pub t: f32,
    pub frozen: bool,
}

pub(crate) fn tick(time: Res<Time>, mut clock: ResMut<Clock>) {
    if !clock.frozen {
        clock.t += time.delta_secs();
    }
}

/// The sky and depth of field, as the owner of the app sets them.
#[derive(Resource, Clone, PartialEq)]
pub struct Stage {
    pub night: bool,
    /// Mild tilt-shift blur in the far views.
    pub dof: bool,
}

impl Default for Stage {
    fn default() -> Self {
        Stage {
            night: false,
            dof: true,
        }
    }
}
