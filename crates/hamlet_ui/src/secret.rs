//! Text fields whose text is drawn as stars.

use bevy::app::{App, PostUpdate};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::With;
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use bevy::picking::Pickable;
use bevy::text::{EditableText, TextColor};
use bevy::ui::AlignItems;
use bevy::ui::widget::Text;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, label, overlay, row, text_field,
};
use bevy_fynix::{Prop, ScopedExt};

use crate::View;
use crate::looks::{pad, well};

pub fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, mask);
}

/// A [`text_field`] for a key or a token, drawn as stars.
pub(crate) fn secret_field(
    value: impl Into<Prop<String>>,
    on_commit: impl Fn(&mut World, String) + Send + Sync + 'static,
) -> impl View {
    // The field takes the room; the stars lie over it.
    row((
        text_field(value, on_commit)
            .grow(1.0)
            .rules(well)
            .tagged(Secret),
        overlay((label("").tagged(Stars).tagged(Pickable::IGNORE),))
            .padding(pad())
            .align(AlignItems::Center)
            .tagged(Pickable::IGNORE),
    ))
}

#[derive(Component)]
struct Secret;

/// The stars over a [`Secret`] field.
#[derive(Component)]
struct Stars;

fn mask(
    fields: Query<(&Children, &ChildOf), With<Secret>>,
    mut inputs: Query<(&EditableText, &mut TextColor)>,
    below: Query<&Children>,
    mut stars: Query<&mut Text, With<Stars>>,
) {
    for (inside, parent) in &fields {
        let Some(typed) = inside.iter().find_map(|&child| {
            let (text, mut ink) = inputs.get_mut(child).ok()?;
            // The editor keeps the text; only the stars are drawn.
            if ink.0 != Color::NONE {
                ink.0 = Color::NONE;
            }
            Some(text.value().to_string().chars().count())
        }) else {
            continue;
        };
        let masked = "*".repeat(typed);
        for each in below.iter_descendants(parent.parent()) {
            if let Ok(mut text) = stars.get_mut(each)
                && text.0 != masked
            {
                text.0.clone_from(&masked);
            }
        }
    }
}
