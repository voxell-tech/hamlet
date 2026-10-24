//! The scene's clock, and the stage the chief sets.

use bevy::prelude::*;

use crate::sky;

/// The clock and the stage, and the order the world's systems run in
/// each frame.
pub fn plugin(app: &mut App) {
    app.init_resource::<Clock>()
        .init_resource::<Stage>()
        .configure_sets(
            Update,
            (
                (
                    RealmSystems::Steer,
                    RealmSystems::Aim,
                    RealmSystems::Act,
                    RealmSystems::Bridge,
                    WorldSystems,
                )
                    .chain(),
                (Step::Clock, Step::Scene, Step::Robots)
                    .chain()
                    .in_set(WorldSystems),
            ),
        )
        .add_systems(
            Update,
            tick.in_set(Step::Clock).before(sky::tell),
        );
}

/// The world's systems, in the order they run each frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldSystems;

/// The steps that come before [`WorldSystems`] each frame, in order,
/// so that the world draws all of them in the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum RealmSystems {
    /// The camera moved by hand, and the stage the chief sets.
    Steer,
    /// The pointer's place over the world, which the chief's hand
    /// acts on.
    Aim,
    /// The chief's deeds on what was aimed: the hand, the things
    /// carried, the clockworks, the mascot and the library.
    Act,
    /// The client's hamlets, agents and roads drawn in the society,
    /// after the chief and the library have taken their sites, and
    /// the view framed on them.
    Bridge,
}

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

/// The sky and depth of field, as the chief sets them.
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
