//! A chord pressed, and the command it runs.

use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::InputFocus;
use bevy::text::EditableText;
use bevy_fynix::shortcut::{Chord, Mods};

use super::{Keys, Standing};
use crate::command::RunCommand;

pub(super) fn press(
    pressed: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    focus: Option<Res<InputFocus>>,
    fields: Query<(), With<EditableText>>,
    mut commands: Commands,
) {
    let typing = focus
        .and_then(|focus| focus.get())
        .is_some_and(|node| fields.contains(node));
    let mods = Mods::held(&pressed);
    for key in pressed.get_just_pressed() {
        let chord = Chord { key: *key, mods };
        // Shift and Alt type letters, so only these may be pressed
        // in a field.
        if typing && !(mods.primary || mods.ctrl || mods.meta) {
            continue;
        }
        if let Some(bound) = keys.0.iter().find(|bound| {
            bound.chord == chord && bound.standing == Standing::Runs
        }) {
            commands.trigger(RunCommand {
                id: bound.command.clone(),
            });
        }
    }
}
