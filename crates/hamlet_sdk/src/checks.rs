//! The checks a signal reads again by.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy_fynix::resource;

/// A check that fires when the resource `R` has changed.
pub fn moved<R: Resource>()
-> impl FnMut(&World) -> bool + Send + Sync + 'static {
    let mut signal = resource::<R, _>(|_| ());
    move |world: &World| signal.changed(world)
}

/// A check that fires when either of two does.
pub fn either(
    mut one: impl FnMut(&World) -> bool + Send + Sync + 'static,
    mut other: impl FnMut(&World) -> bool + Send + Sync + 'static,
) -> impl FnMut(&World) -> bool + Send + Sync + 'static {
    move |world: &World| {
        // Both look, so that neither holds a change over.
        let (one, other) = (one(world), other(world));
        one || other
    }
}
