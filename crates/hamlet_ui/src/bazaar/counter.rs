//! A stall's counter, as the Inspector shows it while the stall is
//! selected in the world: its keeper, what they say, and their wares.

use bevy_fynix::ViewExt;
use bevy_fynix::views::{FrameProps, column};
use hamlet_world::bazaar::Stall;

use crate::AnyView;
use crate::looks::{headed, note, title};

pub(super) fn plugin(_app: &mut bevy::app::App) {}

/// The counter of `stall`.
pub(super) fn counter(stall: Stall) -> AnyView {
    let keeper = stall.keeper();
    column((
        headed(
            super::glyph(stall),
            title(stall.name().to_uppercase()),
        ),
        note(format!("{}, {}", keeper.name, keeper.title)),
        note(keeper.greetings[0]),
        super::wares(stall),
    ))
    .gap(10.0)
    .boxed()
}
