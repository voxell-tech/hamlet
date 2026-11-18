//! The commands a chief can type, listed over the field while what is
//! typed is a slash and the start of one, each pressed to put it in
//! the field.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    self, BehaviorExt, FrameProps, Piece, RichField, button, column,
    frame, label,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm::ActionUsage;
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{bare, small_rules};
use hamlet_ui::{AnyView, View};

use super::{ChatField, field};

/// The most commands listed at once.
const MOST: usize = 6;

/// The text typed in `hub`'s field now, without its chips.
fn typed(world: &World, hub: Entity) -> Option<String> {
    let mut fields = world.try_query::<(&RichField, &ChatField)>()?;
    fields
        .iter(world)
        .find(|(_, field)| field.0 == hub)
        .map(|(field, _)| field.plain().text)
}

/// The commands that start with what is typed in `hub`'s field,
/// while it is a slash and one word.
fn matching(world: &World, hub: Entity) -> Vec<protocol::Action> {
    let Some(typed) = typed(world, hub) else {
        return Vec::new();
    };
    let Some(start) = typed.strip_prefix('/') else {
        return Vec::new();
    };
    if start.contains(char::is_whitespace) {
        return Vec::new();
    }
    let start = start.to_lowercase();
    world.get_resource::<ActionUsage>().map_or_else(
        Vec::new,
        |actions| {
            actions
                .0
                .iter()
                .filter(|action| action.name.starts_with(&start))
                .take(MOST)
                .cloned()
                .collect()
        },
    )
}

/// The list over `hub`'s field, while anything matches.
pub(super) fn list(hub: Entity) -> impl View {
    keyed(
        derived(move |world: &World| matching(world, hub))
            .when(every_frame()),
        move |actions| {
            if actions.is_empty() {
                return frame().boxed();
            }
            column(
                actions
                    .iter()
                    .cloned()
                    .map(|action| line(hub, action))
                    .collect::<Vec<_>>(),
            )
            .gap(0.0)
            .padding(UiRect::bottom(px(6.0)))
            .boxed()
        },
    )
    .within(bare())
}

/// A command as it is typed, with what it does under it.
fn line(hub: Entity, action: protocol::Action) -> AnyView {
    let name = action.name;
    button(
        column((
            label(action.usage)
                .bold(true)
                .wrap(false)
                .rules(small_rules),
            label(action.description)
                .tone(Tone::Dim)
                .rules(small_rules),
        ))
        .gap(2.0)
        .align(AlignItems::FlexStart),
    )
    .padding(UiRect::axes(px(6.0), px(4.0)))
    .justify(JustifyContent::FlexStart)
    .rules(views::tint)
    .on_activate(move |world| {
        field::set(
            world,
            hub,
            vec![Piece::Text(format!("/{name} "))],
        );
    })
    .boxed()
}
