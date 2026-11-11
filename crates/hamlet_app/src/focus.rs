//! Fields that take the keyboard as soon as they show, as a name to
//! type in after Rename, so it needs no click first.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::Added;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::text::{EditableText, TextEdit};

/// Marks a field, or what holds one, to take the keyboard when it
/// shows.
#[derive(Component, Clone, Copy)]
pub(crate) struct TakesFocus;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, take);
}

fn take(
    shown: Query<Entity, Added<TakesFocus>>,
    mut commands: Commands,
) {
    for entity in &shown {
        commands.queue(move |world: &mut World| {
            let Some(field) = editable(world, entity) else {
                return;
            };
            world
                .resource_mut::<InputFocus>()
                .set(field, FocusCause::Pressed);
            if let Some(mut text) =
                world.get_mut::<EditableText>(field)
            {
                text.queue_edit(TextEdit::SelectAll);
            }
        });
    }
}

/// The first entity at or under `entity` that is typed in.
fn editable(world: &World, entity: Entity) -> Option<Entity> {
    if world.get::<EditableText>(entity).is_some() {
        return Some(entity);
    }
    let children = world.get::<Children>(entity)?;
    children.iter().find_map(|child| editable(world, *child))
}
