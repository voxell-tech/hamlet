//! A hamlet's budget lines from the server.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{UiRect, px};
use bevy_fynix::views::{FrameProps, column};
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_client::realm::Treasury;

use super::heading;
use crate::View;
use crate::icons::Glyph;
use crate::looks::{bare, note};
use crate::roll::hamlets_moved;

/// The spending reported for a hamlet.
pub(super) fn record_of(hamlet: Entity) -> impl View {
    let told = derived(move |world: &World| {
        world
            .get::<Treasury>(hamlet)
            .map(|treasury| treasury.0.lines.clone())
            .unwrap_or_default()
    })
    .when(hamlets_moved());
    keyed(told, |lines| {
        let mut parts = vec![heading(Glyph::Meter, "THE BUDGET")];
        parts.extend(
            lines.iter().map(|line| note(line.clone()).boxed()),
        );
        column(parts).gap(8.0).padding(UiRect::top(px(4.0))).boxed()
    })
    .within(bare())
}
