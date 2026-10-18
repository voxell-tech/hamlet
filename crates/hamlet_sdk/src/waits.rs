//! How many things wait for the chief.

use bevy::prelude::*;
use hamlet_core::oversight::Waiting;

/// Keeps [`Waits`] in step with the things that wait.
pub fn plugin(app: &mut App) {
    app.init_resource::<Waits>().add_systems(Update, count);
}

/// The number of things that wait for the chief.
#[derive(Resource, Default, PartialEq, Clone, Copy, Debug)]
pub struct Waits(pub usize);

fn count(
    waiting: Query<(), With<Waiting>>,
    mut waits: ResMut<Waits>,
) {
    let now = waiting.iter().count();
    if waits.0 != now {
        waits.0 = now;
    }
}
