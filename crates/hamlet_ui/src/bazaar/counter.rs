//! A stall as the Inspector shows it while the stall is selected in
//! the world: what it sells, with nothing over it.

use bevy::app::App;
use bevy_fynix::views::{FrameProps, column, scroll};
use hamlet_world::bazaar::Stall;

use crate::looks::Fills;
use crate::{AnyView, Cx};

pub(super) fn plugin(_app: &mut App) {}

/// The wares of `stall`, scrolling in the Inspector.
pub(super) fn counter(stall: Stall) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        cx.build(
            column((scroll((super::wares(stall),)).fills_height(),))
                .fills_height()
                .gap(0.0),
        )
    })
}
