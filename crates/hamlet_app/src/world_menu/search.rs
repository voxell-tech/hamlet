//! The search field of a popup's list, which narrows the list as it
//! is typed in rather than once Enter is pressed.

use std::sync::Arc;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::Changed;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use bevy::picking::Pickable;
use bevy::text::EditableText;
use bevy::ui::{
    AlignItems, Display, JustifyContent, PositionType, UiRect, Val,
    percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row, text_field,
};
use bevy_fynix::{ScopedExt, derived};
use hamlet_ui::styles::{ICON, text_input};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{View, with_cx};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, follow);
}

/// What a search field does with each change of its text.
#[derive(Component, Clone)]
struct Typed(Arc<dyn Fn(&mut World, String) + Send + Sync>);

/// Hands each search field's new text to its [`Typed`].
fn follow(
    texts: Query<(&EditableText, &ChildOf), Changed<EditableText>>,
    fields: Query<&Typed>,
    mut commands: Commands,
) {
    for (text, child) in &texts {
        let Ok(typed) = fields.get(child.parent()) else {
            continue;
        };
        let (typed, text) =
            (typed.0.clone(), text.value().to_string());
        commands.queue(move |world: &mut World| typed(world, text));
    }
}

/// A search field after a lens, with `word` in it while it is empty;
/// `read` gives the text searched for, and `write` takes it as it is
/// typed.
pub(super) fn search(
    word: &'static str,
    read: impl Fn(&World) -> String + Send + Sync + 'static,
    write: impl Fn(&mut World, String) + Send + Sync + 'static,
) -> impl View {
    let field = column((
        text_field(String::new(), |_, _| {})
            .width(percent(100.0))
            .rules(text_input)
            .tagged(Typed(Arc::new(write))),
        // The word that stands in an empty field, which the pointer
        // passes through to the field.
        column((label(word).tone(Tone::Faint).wrap(false),))
            .position(PositionType::Absolute)
            .inset(UiRect::new(px(8.0), Val::Auto, px(0.0), px(0.0)))
            .justify(JustifyContent::Center)
            .display(
                derived(move |world: &World| {
                    if read(world).is_empty() {
                        Display::Flex
                    } else {
                        Display::None
                    }
                })
                .when(bevy_fynix::every_frame()),
            )
            .tagged(Pickable::IGNORE),
    ))
    .grow(1.0)
    .min_width(px(0.0));
    with_cx(move |cx| {
        row((glyph(Glyph::Lens, ICON).tone(Tone::Faint), field))
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
    })
}
