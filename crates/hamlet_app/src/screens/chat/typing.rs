//! Typing with the pointer over a Hub's chat, which goes to its
//! field wherever the keyboard was.

use bevy::ecs::message::{MessageCursor, Messages};
use bevy::ecs::query::{Or, With};
use bevy::ecs::system::Local;
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::text::EditableText;
use bevy_fynix::views::RichField;

use super::ChatField;
use crate::hub::{self, Showing};
use crate::menu::OpenMenu;
use crate::screens::viewer::Viewing;
use crate::world_menu::Open;

/// Gives the field of the Hub under the pointer the keyboard when a
/// letter is typed and no text field has it, before the key is
/// handed to whatever has the focus.
pub(super) fn type_into_field(
    world: &mut World,
    mut cursor: Local<MessageCursor<KeyboardInput>>,
) {
    let held = world.resource::<ButtonInput<Key>>();
    let chord = [Key::Control, Key::Super, Key::Alt]
        .into_iter()
        .any(|key| held.pressed(key));
    let focused =
        world.get_resource::<InputFocus>().and_then(InputFocus::get);
    let typed = world
        .get_resource::<Messages<KeyboardInput>>()
        .is_some_and(|keys| {
            cursor.read(keys).any(|key| {
                key.state.is_pressed()
                    && key.text.as_ref().is_some_and(|text| {
                        // A space belongs to a button that has the
                        // focus.
                        text.chars().any(|c| {
                            !c.is_control()
                                && (focused.is_none()
                                    || !c.is_whitespace())
                        })
                    })
            })
        });
    if chord || !typed {
        return;
    }
    let in_field = focused.is_some_and(|node| {
        let mut fields = world
            .query_filtered::<(), Or<(With<EditableText>, With<RichField>)>>();
        fields.get(world, node).is_ok()
    });
    let covered = world
        .get_resource::<Open>()
        .is_some_and(|open| *open != Open::Nothing)
        || world
            .get_resource::<OpenMenu>()
            .is_some_and(OpenMenu::is_open)
        || world
            .get_resource::<Viewing>()
            .is_some_and(Viewing::is_open);
    if in_field || covered {
        return;
    }
    let Some(hub) = hub::under_pointer(world) else {
        return;
    };
    if world.get::<Showing>(hub) != Some(&Showing::Thread) {
        return;
    }
    let mut fields =
        world.query::<(bevy::ecs::entity::Entity, &ChatField)>();
    let Some(field) = fields
        .iter(world)
        .find(|(_, field)| field.0 == hub)
        .map(|(node, _)| node)
    else {
        return;
    };
    hub::activate(world, hub);
    world
        .resource_mut::<InputFocus>()
        .set(field, FocusCause::Pressed);
}
