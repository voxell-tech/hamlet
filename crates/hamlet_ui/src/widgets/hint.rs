//! The words an empty field shows to say what goes in it.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::{With, Without};
use bevy::ecs::system::{Commands, Query};
use bevy::picking::Pickable;
use bevy::text::EditableText;
use bevy::ui::{
    AlignItems, Display, JustifyContent, Node, PositionType, UiRect,
    Val, percent, px,
};
use bevy_fynix::Prop;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, RichField, column, label, row,
};

use super::icons::{Glyph, glyph};
use crate::styles::ICON;
use crate::{View, with_cx};

pub fn plugin(app: &mut App) {
    app.add_systems(Update, show);
}

/// A field whose [`Hint`] is shown while it is empty.
#[derive(Component)]
struct Hinted;

/// The words over a [`Hinted`] field.
#[derive(Component)]
struct Hint;

/// `field` with `word` over it while it is empty. The pointer passes
/// through the word to the field.
pub fn hinted(
    field: impl View,
    word: impl Into<Prop<String>>,
) -> impl View {
    let word = word.into();
    with_cx(move |cx| {
        let faint = cx.theme().hint_opacity();
        column((
            column((field.tagged(Hinted),)).width(percent(100.0)),
            column((label(word)
                .tone(Tone::Dim)
                .opacity(faint)
                .wrap(false),))
            .position(PositionType::Absolute)
            .inset(UiRect::new(px(8.0), Val::Auto, px(0.0), px(0.0)))
            .justify(JustifyContent::Center)
            .tagged(Hint),
        ))
        .grow(1.0)
        .min_width(px(0.0))
    })
}

/// `field` behind a lens, with `word` in it while it is empty.
pub fn search(
    field: impl View,
    word: impl Into<Prop<String>>,
) -> impl View {
    let field = hinted(field, word);
    with_cx(move |cx| {
        row((glyph(Glyph::Lens, ICON).tone(Tone::Faint), field))
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
            .grow(1.0)
            .min_width(px(0.0))
    })
}

/// Shows each hint while its field is empty, and lets the pointer
/// through it.
fn show(
    mut commands: Commands,
    fields: Query<(Entity, &ChildOf), With<Hinted>>,
    texts: Query<&EditableText>,
    rich: Query<&RichField>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    mut hints: Query<&mut Node, With<Hint>>,
    solid: Query<(), Without<Pickable>>,
) {
    for (field, holder) in &fields {
        let empty = match rich.get(field) {
            Ok(rich) => rich.is_empty(),
            Err(_) => children
                .iter_descendants(field)
                .find_map(|part| texts.get(part).ok())
                .is_none_or(|text| text.value() == ""),
        };
        let display =
            if empty { Display::Flex } else { Display::None };
        // The field's holder and its hint are side by side.
        let Some(beside) = parents
            .get(holder.parent())
            .ok()
            .and_then(|around| children.get(around.parent()).ok())
        else {
            continue;
        };
        for hint in beside.iter().copied() {
            let Ok(mut node) = hints.get_mut(hint) else {
                continue;
            };
            if node.display != display {
                node.display = display;
            }
            for part in std::iter::once(hint)
                .chain(children.iter_descendants(hint))
                .filter(|part| solid.contains(*part))
            {
                commands.entity(part).try_insert(Pickable::IGNORE);
            }
        }
    }
}
