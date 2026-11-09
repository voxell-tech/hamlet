//! The commands a chief can type, listed over the field while what is
//! typed is a slash and the start of one, each pressed to put it in
//! the field.

use bevy::ecs::query::With;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    self, BehaviorExt, FrameProps, Piece, RichField, button, column,
    frame, label,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm::DeedUsage;
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{bare, small_rules};
use hamlet_ui::{AnyView, View};

use super::{ChatField, field};

/// The most commands listed at once.
const MOST: usize = 6;

/// The text typed in the chat's field now, without its chips.
fn typed(world: &World) -> Option<String> {
    let mut fields =
        world.try_query_filtered::<&RichField, With<ChatField>>()?;
    fields.iter(world).next().map(|field| field.plain().text)
}

/// The commands that start with what is typed, while it is a slash
/// and one word.
fn matching(world: &World) -> Vec<protocol::Deed> {
    let Some(typed) = typed(world) else {
        return Vec::new();
    };
    let Some(start) = typed.strip_prefix('/') else {
        return Vec::new();
    };
    if start.contains(char::is_whitespace) {
        return Vec::new();
    }
    let start = start.to_lowercase();
    world
        .get_resource::<DeedUsage>()
        .map_or_else(Vec::new, |deeds| {
            deeds
                .0
                .iter()
                .filter(|deed| deed.name.starts_with(&start))
                .take(MOST)
                .cloned()
                .collect()
        })
}

/// The list over the field, while anything matches.
pub(super) fn list() -> impl View {
    keyed(derived(matching).when(every_frame()), |deeds| {
        if deeds.is_empty() {
            return frame().boxed();
        }
        column(deeds.iter().cloned().map(line).collect::<Vec<_>>())
            .gap(0.0)
            .padding(UiRect::bottom(px(6.0)))
            .boxed()
    })
    .within(bare())
}

/// A command as it is typed, with what it does under it.
fn line(deed: protocol::Deed) -> AnyView {
    let name = deed.name;
    button(
        column((
            label(deed.usage)
                .bold(true)
                .wrap(false)
                .rules(small_rules),
            label(deed.description)
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
        field::set(world, vec![Piece::Text(format!("/{name} "))]);
    })
    .boxed()
}
