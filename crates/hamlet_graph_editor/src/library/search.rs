//! A search field: a magnifier, the field, and what it searches
//! written in it while it is empty. Each change to the text is handed
//! to the field's owner.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::{With, Without};
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use bevy::picking::Pickable;
use bevy::text::{EditableText, TextEditChange};
use bevy::ui::{
    AlignItems, Display, Node, PositionType, UiRect, Val, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt};
use hamlet_ui::styles::{ICON, note, text_input};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, with_cx};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, listen);
}

/// What a change to a search's text runs.
pub type Searched = fn(&mut World, String);

/// A search's field, with what runs as its text changes.
#[derive(Component)]
struct Field(Searched);

/// The words written in a search's field while it is empty.
#[derive(Component)]
struct Hint;

/// Marks a field that is heard.
#[derive(Component)]
struct Heard;

/// A search field `width` wide that says `hint` while empty and runs
/// `write` with its text each time it changes.
pub fn search(
    hint: &'static str,
    width: f32,
    write: Searched,
) -> AnyView {
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        row((
            glyph(Glyph::Lens, ICON).tone(Tone::Dim),
            column((
                text_field(String::new(), |_, _| {})
                    .width(px(width))
                    .rules(text_input)
                    .tagged(Field(write)),
                row((note(hint).tagged(Pickable::IGNORE),))
                    .position(PositionType::Absolute)
                    .inset(UiRect::new(
                        px(10.0),
                        Val::Auto,
                        px(0.0),
                        px(0.0),
                    ))
                    .align(AlignItems::Center)
                    .tagged(Pickable::IGNORE)
                    .tagged(Hint),
            ))
            .gap(0.0),
        ))
        .gap(gap / 2.0)
        .align(AlignItems::Center)
    })
    .boxed()
}

/// Hears each search's field once it is built: its text goes to its
/// owner, and its hint shows only while it is empty.
fn listen(
    mut commands: Commands,
    fields: Query<
        (Entity, &Field, &Children, &ChildOf),
        Without<Heard>,
    >,
    places: Query<&Children>,
    hints: Query<(), With<Hint>>,
) {
    for (field, Field(write), children, ChildOf(place)) in &fields {
        let Some(&input) = children.first() else {
            continue;
        };
        let Some(hint) = places.get(*place).ok().and_then(|kids| {
            kids.iter().copied().find(|kid| hints.contains(*kid))
        }) else {
            continue;
        };
        let write = *write;
        commands.entity(field).insert(Heard);
        commands.entity(input).observe(
            move |change: On<TextEditChange>,
                  texts: Query<&EditableText>,
                  mut nodes: Query<&mut Node>,
                  mut commands: Commands| {
                let Ok(text) = texts.get(change.event_target())
                else {
                    return;
                };
                let typed = text.value().to_string();
                if let Ok(mut node) = nodes.get_mut(hint) {
                    node.display = if typed.is_empty() {
                        Display::Flex
                    } else {
                        Display::None
                    };
                }
                commands.queue(move |world: &mut World| {
                    write(world, typed);
                });
            },
        );
    }
}
