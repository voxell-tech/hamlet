//! The Artisans stall's wares.

use bevy_fynix::ViewExt;
use hamlet_world::bazaar::Stall;

use crate::AnyView;
use crate::looks::note;

/// What the stall sells.
pub(super) fn page() -> AnyView {
    note(Stall::Artisans.sells()).boxed()
}
