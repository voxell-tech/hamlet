//! The scene's clock, and the stage its owner sets.

use bevy::prelude::*;

use crate::keeper::KState;
use crate::sky;

/// The clock and the stage, and the order the world's systems run in
/// each frame.
pub fn plugin(app: &mut App) {
    app.init_resource::<Clock>()
        .init_resource::<Stage>()
        .configure_sets(
            Update,
            (Step::Clock, Step::Scene, Step::Robots)
                .chain()
                .in_set(WorldSystems),
        )
        .add_systems(
            Update,
            tick.in_set(Step::Clock).before(sky::tell),
        );
}

/// The world's systems, in the order they run each frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldSystems;

/// The steps of [`WorldSystems`], in order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Step {
    Clock,
    /// A world's following of the stage: spawning, placing and
    /// lighting.
    Scene,
    /// The robots take their poses.
    Robots,
}

/// Scene time. Stills freeze it so that every run renders the same
/// frame.
#[derive(Resource, Default)]
pub struct Clock {
    pub t: f32,
    pub frozen: bool,
}

fn tick(time: Res<Time>, mut clock: ResMut<Clock>) {
    if !clock.frozen {
        clock.t += time.delta_secs();
    }
}

/// The sky and the mayor's state, as the owner of the app sets them.
#[derive(Resource, Clone, PartialEq)]
pub struct Stage {
    pub night: bool,
    pub kstate: KState,
    /// Mild tilt-shift blur in the far views.
    pub dof: bool,
}

impl Default for Stage {
    fn default() -> Self {
        Stage {
            night: false,
            kstate: KState::Rest,
            dof: true,
        }
    }
}

impl Stage {
    /// True while the mayor moves, so frames must keep coming.
    pub fn animating(&self) -> bool {
        self.kstate.animates()
    }
}
