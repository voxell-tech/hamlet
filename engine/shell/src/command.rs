//! Commands and the keys bound to them. A command is an entity with
//! a [`Command`]; whoever owns it observes [`RunCommand`] on it. Menu
//! entries and shortcuts point at a command by its id.

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, EntityCommands, Query, Res};
use bevy::ecs::world::World;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input::{ButtonInput, ButtonState};
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::text::EditableText;
use bevy::window::Window;
use bevy_fynix::shortcut::{Chord, Mods};
use hamlet_engine_api::CommandId;
use hamlet_engine_api::id::ENGINE;

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(press);
}

/// Something the person can run, from a menu or by a key.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct Command {
    pub id: CommandId,
    /// What a menu calls it.
    pub label: String,
}

/// Runs the command it is triggered on.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct RunCommand {
    pub entity: Entity,
}

/// A key bound to a command.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct Shortcut {
    pub chord: Chord,
    pub command: CommandId,
}

/// A command's entry in one of the bar's menus.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct MenuEntry {
    /// The menu's title.
    pub menu: String,
    pub command: CommandId,
}

/// Control: the primary key off macOS, where Command is.
pub const CTRL: Mods = if cfg!(target_os = "macos") {
    Mods::CTRL
} else {
    Mods::PRIMARY
};

/// The engine's own command `name`, which the engine names well.
pub fn engine_id(name: &str) -> CommandId {
    CommandId::new(ENGINE, name).expect("a valid id")
}

/// Spawns the command `id`, with its entry in `menu` and its keys,
/// which go with it. Its owner observes [`RunCommand`] on what this
/// returns.
pub fn engine_command<'a>(
    commands: &'a mut Commands,
    id: CommandId,
    label: &str,
    menu: Option<&str>,
    keys: &[Chord],
) -> EntityCommands<'a> {
    let mut command = commands.spawn(Command {
        id: id.clone(),
        label: label.into(),
    });
    command.with_children(|kids| {
        if let Some(menu) = menu {
            kids.spawn(MenuEntry {
                menu: menu.into(),
                command: id.clone(),
            });
        }
        for &chord in keys {
            kids.spawn(Shortcut {
                chord,
                command: id.clone(),
            });
        }
    });
    command
}

/// Runs the command `id`, if there is one.
pub fn run(world: &mut World, id: &CommandId) {
    let found = world.try_query::<(Entity, &Command)>().and_then(
        |mut all| {
            all.iter(world)
                .find(|(_, command)| command.id == *id)
                .map(|(entity, _)| entity)
        },
    );
    if let Some(entity) = found {
        world.trigger(RunCommand { entity });
    }
}

/// Runs the command a key press is bound to. A bare key in a text
/// field is the field's to type.
fn press(
    event: On<FocusedInput<KeyboardInput>>,
    windows: Query<(), With<Window>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    focus: Res<InputFocus>,
    typing: Query<(), With<EditableText>>,
    shortcuts: Query<&Shortcut>,
    commands: Query<(Entity, &Command)>,
    mut out: Commands,
) {
    let key = &event.input;
    // Seen once, where it starts: at the focus, or at the window
    // when nothing holds the focus.
    let first = match focus.get() {
        Some(held) => event.event_target() == held,
        None => windows.contains(event.event_target()),
    };
    if !first || key.state != ButtonState::Pressed || key.repeat {
        return;
    }
    let mods = keys.as_deref().map_or(Mods::NONE, Mods::held);
    let plain = !(mods.primary || mods.ctrl || mods.alt || mods.meta);
    if plain && focus.get().is_some_and(|held| typing.contains(held))
    {
        return;
    }
    let chord = Chord {
        key: key.key_code,
        mods,
    };
    let bound = shortcuts
        .iter()
        .filter(|shortcut| shortcut.chord == chord)
        .find_map(|shortcut| {
            commands
                .iter()
                .find(|(_, command)| command.id == shortcut.command)
        });
    if let Some((entity, _)) = bound {
        out.trigger(RunCommand { entity });
    }
}
