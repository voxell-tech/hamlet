//! Change checks for views read from several resources at once.

use std::any::Any;

use hamlet_engine_sdk::ui::{World, resource};

/// A check that fires when the resource `R` changed.
pub fn moved<R: Any + Send + Sync>()
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
