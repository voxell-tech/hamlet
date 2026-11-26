//! The line at the end of a chat while its earlier messages are
//! summarized.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Display, UiRect, px};
use hamlet_core::client::chats::Status;
use hamlet_core::fynix::tokens::Tone;
use hamlet_core::fynix::views::{FrameProps, label, row};
use hamlet_core::fynix::{ScopedExt, derived, every_frame};
use hamlet_core::ui::styles::small_rules;
use hamlet_core::ui::widgets::shimmer::shimmer;
use hamlet_core::ui::{View, with_cx};

use crate::convo;

/// Whether the earlier messages of `hub`'s chat are being summarized.
fn compacting(world: &World, hub: Entity) -> bool {
    convo::of(world, hub)
        .and_then(|convo| world.get::<Status>(convo.chat))
        .is_some_and(|status| status.compacting)
}

/// "Compacting the earlier messages", in `hub`.
pub(super) fn line(hub: Entity) -> impl View {
    with_cx(move |cx| {
        let shown = derived(move |world: &World| {
            match compacting(world, hub) {
                true => Display::Flex,
                false => Display::None,
            }
        })
        .when(every_frame());
        row((
            shimmer("Compacting"),
            label(" the earlier messages")
                .wrap(false)
                .tone(Tone::Dim),
        ))
        .gap(0.0)
        .padding(UiRect::horizontal(px(cx.theme().space.gap)))
        .align(AlignItems::Center)
        .display(shown)
        .rules(small_rules)
    })
}
