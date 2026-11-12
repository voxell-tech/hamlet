//! The field a message is typed in: text with the chips of what is
//! attached to it.

use bevy::ecs::change_detection::Mut;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy_fynix::views::{Piece, Plain, RichField};

use super::{ChatField, Draft};

/// Runs `edit` on the field of the chat.
pub(super) fn with_field<R>(
    world: &mut World,
    edit: impl FnOnce(Mut<RichField>) -> R,
) -> Option<R> {
    let mut fields =
        world.query_filtered::<&mut RichField, With<ChatField>>();
    fields.iter_mut(world).next().map(edit)
}

/// The text of the field, with a place for each chip in it.
pub(super) fn typed(world: &mut World) -> Option<Plain> {
    with_field(world, |field| field.plain())
}

/// Puts `pieces` in the field, with the caret at their end, and gives
/// it the focus.
pub(super) fn set(world: &mut World, pieces: Vec<Piece>) {
    with_field(world, |mut field| field.set_content(pieces));
    world.resource_mut::<Draft>().focus = true;
}

/// Gives the field the focus when it was asked for.
pub fn refocus(
    mut draft: ResMut<Draft>,
    fields: Query<Entity, With<ChatField>>,
    mut focus: ResMut<InputFocus>,
) {
    if !draft.focus {
        return;
    }
    draft.focus = false;
    if let Some(field) = fields.iter().next() {
        focus.set(field, FocusCause::Pressed);
    }
}
