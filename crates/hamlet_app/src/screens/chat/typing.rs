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

/// The keys that make a chord of the one pressed with them.
const MODIFIERS: [Key; 3] = [Key::Control, Key::Super, Key::Alt];

/// Gives the field of the Hub under the pointer the keyboard when a
/// letter is typed and no text field has it, before the key is
/// handed to whatever has the focus.
pub(super) fn type_into_field(
    world: &mut World,
    mut cursor: Local<MessageCursor<KeyboardInput>>,
) {
    let focused =
        world.get_resource::<InputFocus>().and_then(InputFocus::get);
    let keys = world
        .get_resource::<Messages<KeyboardInput>>()
        .map(|keys| cursor.read(keys).cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let held = world.get_resource::<ButtonInput<Key>>();
    // Each modifier as it was before the first of its presses and
    // releases in these keys, then followed key by key, so that a
    // letter is judged by the modifiers held when it was typed.
    let mut mods = MODIFIERS.map(|modifier| {
        keys.iter()
            .find(|key| key.logical_key == modifier)
            .map_or_else(
                || held.is_some_and(|held| held.pressed(modifier)),
                |key| !key.state.is_pressed(),
            )
    });
    let mut typed = false;
    for key in &keys {
        if let Some(at) =
            MODIFIERS.iter().position(|m| *m == key.logical_key)
        {
            mods[at] = key.state.is_pressed();
            continue;
        }
        typed |= key.state.is_pressed()
            && !mods.contains(&true)
            && key.text.as_ref().is_some_and(|text| {
                // A space belongs to a button that has the focus.
                text.chars().any(|c| {
                    !c.is_control()
                        && (focused.is_none() || !c.is_whitespace())
                })
            });
    }
    if !typed {
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
