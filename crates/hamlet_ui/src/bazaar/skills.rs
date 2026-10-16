//! The Skills stall's wares.

use bevy_fynix::ViewExt;
use hamlet_world::bazaar::Stall;

use crate::AnyView;
use crate::looks::note;

pub(super) fn plugin(_app: &mut bevy::app::App) {}

/// What the stall sells.
pub(super) fn page() -> AnyView {
    note(Stall::Skills.sells()).boxed()
}
