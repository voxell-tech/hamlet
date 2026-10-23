//! Change checks for views read from several resources at once.

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::resource;

/// A check that fires when the resource `R` changed.
pub fn moved<R: Resource>()
-> impl FnMut(&World) -> bool + Send + Sync + 'static {
    let mut seen = resource::<R, _>(|_| ());
    move |world: &World| seen.changed(world)
}

/// A check that fires when either `a` or `b` does. Both are asked,
/// so that neither is left unread.
pub fn either(
    mut a: impl FnMut(&World) -> bool + Send + Sync + 'static,
    mut b: impl FnMut(&World) -> bool + Send + Sync + 'static,
) -> impl FnMut(&World) -> bool + Send + Sync + 'static {
    move |world: &World| a(world) | b(world)
}
