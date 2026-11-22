//! The search field of a popup's list, which narrows the list as it
//! is typed in rather than once Enter is pressed.

use std::sync::Arc;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::Changed;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy_fynix::views::{BehaviorExt, text_field};
use bevy_fynix::{Binding, ScopedExt};

use crate::View;
use crate::styles::text_input;
use crate::widgets::hint;

pub fn plugin(app: &mut App) {
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

/// A search field after a lens, holding `text` at first and `word`
/// while it is empty; `write` takes the text as it is typed.
pub fn search(
    word: &'static str,
    text: String,
    write: impl Fn(&mut World, String) + Send + Sync + 'static,
) -> impl View {
    hint::search(
        text_field(Binding::new(text, |_, _| {}))
            .rules(text_input)
            .tagged(Typed(Arc::new(write))),
        word,
    )
}
