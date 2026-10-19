//! The field a message is typed in: its placeholder, and Escape.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Has, With};
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::picking::Pickable;
use bevy::text::EditableText;
use bevy::ui::Node;

use super::{ChatField, Draft};
use crate::platform::show;

/// The editable text inside the field.
fn typing(
    root: &Query<&Children, With<ChatField>>,
    texts: &Query<&EditableText>,
) -> Option<Entity> {
    root.iter()
        .flat_map(|children| children.iter().copied())
        .find(|child| texts.contains(*child))
}

/// Keeps what was typed when Escape is pressed in the field.
pub fn keep_typed(
    key: On<FocusedInput<KeyboardInput>>,
    mut draft: ResMut<Draft>,
    root: Query<&Children, With<ChatField>>,
    texts: Query<&EditableText>,
) {
    let Some(input) = typing(&root, &texts) else {
        return;
    };
    if key.focused_entity != input
        || !key.input.state.is_pressed()
        || key.input.logical_key != Key::Escape
    {
        return;
    }
    // The field puts back its bound text on Escape, so what was
    // typed is made that text.
    if let Ok(text) = texts.get(input) {
        draft.text = text.value().to_string();
    }
}

/// The most lines the field grows to before it scrolls.
const MOST_LINES: usize = 8;

/// Grows each field with the lines in it, so a quote and the words
/// after it are both seen.
pub fn grow(
    root: Query<&Children, With<ChatField>>,
    mut texts: Query<&mut EditableText>,
) {
    let inputs = root
        .iter()
        .flat_map(|children| children.iter().copied())
        .collect::<Vec<_>>();
    for input in inputs {
        let Ok(mut text) = texts.get_mut(input) else {
            continue;
        };
        let lines = text
            .editor
            .try_layout()
            .map_or(1, |layout| layout.len().clamp(1, MOST_LINES));
        // One line keeps the height the field is drawn at.
        let want = (lines > 1).then_some(lines as f32);
        if text.visible_lines != want {
            text.visible_lines = want;
        }
    }
}

/// The words an empty field shows to say what can be typed.
#[derive(Component)]
pub struct Placeholder;

/// Shows the placeholder only in an empty field, and keeps the
/// pointer passing through it to the field.
pub fn show_placeholder(
    mut commands: Commands,
    root: Query<&Children, With<ChatField>>,
    texts: Query<&EditableText>,
    mut words: Query<
        (Entity, &mut Node, Has<Pickable>),
        With<Placeholder>,
    >,
    under: Query<&Children>,
) {
    let blank = typing(&root, &texts)
        .and_then(|input| texts.get(input).ok())
        .is_none_or(|text| text.value() == "");
    for (entity, mut node, passes) in &mut words {
        show(&mut node, blank);
        if !passes {
            commands.entity(entity).try_insert(Pickable::IGNORE);
            for child in under.iter_descendants(entity) {
                commands.entity(child).try_insert(Pickable::IGNORE);
            }
        }
    }
}
