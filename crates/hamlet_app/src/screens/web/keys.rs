//! The keys of the Web panels: the page has them while it holds the
//! focus, except the window's own chords, and a Web panel's commands
//! go to the page under the pointer.

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::text::EditableText;
use bevy_cef::prelude::{
    CefKeyboardFilter, FocusedWebview, KeyboardDeliverSet,
    ModifiersState, RequestGoBack, RequestGoForward, RequestReload,
};
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandList, CommandSpec, Invoke, Keymap, Mods,
    ShortcutAppExt,
};

use super::page::{self, Page};
use super::panel::{AddressField, PanelId, SCOPE};

/// The keys a page keeps when the window has a command on them too:
/// editing, which the page's text needs.
const EDITING: [KeyCode; 5] = [
    KeyCode::KeyA,
    KeyCode::KeyC,
    KeyCode::KeyV,
    KeyCode::KeyX,
    KeyCode::KeyZ,
];

pub(super) fn plugin(app: &mut App) {
    let command = |id, label, run| CommandSpec {
        id: CommandId(id),
        label,
        scope: SCOPE,
        run,
        enabled: |_| true,
        repeat: false,
    };
    let primary = |key| Chord {
        key,
        mods: Mods::PRIMARY,
    };
    app.add_command(
        command("web.address", "Go to the address", to_address),
        &[primary(KeyCode::KeyL)],
    )
    .add_command(
        command("web.reload", "Reload", |world, invoke| {
            if let Some(webview) = page_of(world, invoke) {
                world.trigger(RequestReload { webview });
            }
        }),
        &[primary(KeyCode::KeyR)],
    )
    .add_command(
        command("web.back", "Back", |world, invoke| {
            if let Some(webview) = page_of(world, invoke) {
                world.trigger(RequestGoBack { webview });
            }
        }),
        &[primary(KeyCode::BracketLeft)],
    )
    .add_command(
        command("web.forward", "Forward", |world, invoke| {
            if let Some(webview) = page_of(world, invoke) {
                world.trigger(RequestGoForward { webview });
            }
        }),
        &[primary(KeyCode::BracketRight)],
    )
    .add_observer(keep_typed)
    .add_systems(
        Update,
        (follow_focus, hold_chords)
            .chain()
            .before(KeyboardDeliverSet),
    );
}

/// The panel a command was run over.
fn panel_of(world: &World, invoke: Invoke) -> Option<PanelId> {
    world.get::<PanelId>(invoke.target?).cloned()
}

/// The page of the panel a command was run over.
fn page_of(world: &mut World, invoke: Invoke) -> Option<Entity> {
    let panel = panel_of(world, invoke)?;
    page::of(world, &panel)
}

/// Gives the keys to the address field of the panel a command was run
/// over.
fn to_address(world: &mut World, invoke: Invoke) {
    let Some(panel) = invoke.target else {
        return;
    };
    let text = world
        .query_filtered::<(Entity, &Children), With<AddressField>>()
        .iter(world)
        .filter(|&(field, _)| is_under(world, field, panel))
        .flat_map(|(_, children)| children.iter().copied())
        .find(|&kid| world.get::<EditableText>(kid).is_some());
    if let Some(text) = text {
        world
            .resource_mut::<InputFocus>()
            .set(text, FocusCause::Navigated);
    }
}

/// Whether `node` is under `above`.
fn is_under(world: &World, node: Entity, above: Entity) -> bool {
    let mut at = node;
    while let Some(parent) = world.get::<ChildOf>(at) {
        at = parent.parent();
        if at == above {
            return true;
        }
    }
    false
}

/// Keeps the page the engine sends keys to and the window's focus as
/// one: a press on a page gives it the focus, and the focus going
/// anywhere else takes the keys from the page.
fn follow_focus(
    mut focus: ResMut<InputFocus>,
    mut webview: ResMut<FocusedWebview>,
    pages: Query<(), With<Page>>,
    mut last: Local<Option<Entity>>,
) {
    if webview.is_changed()
        && let Some(page) = webview.0
        && pages.contains(page)
        && (*last != Some(page) || focus.get() != Some(page))
    {
        focus.set(page, FocusCause::Pressed);
    } else if webview.0.is_some() && focus.get() != webview.0 {
        webview.0 = None;
    }
    *last = webview.0;
}

/// Whether the window, rather than a page, answers `chord`: one with
/// a modifier other than Shift that a command is bound to, except the
/// page's editing keys.
fn is_windows(
    chord: Chord,
    list: &CommandList,
    keymap: &Keymap,
) -> bool {
    let Mods {
        primary,
        alt,
        ctrl,
        meta,
        ..
    } = chord.mods;
    if !(primary || alt || ctrl || meta) {
        return false;
    }
    if primary
        && !(alt || ctrl || meta)
        && EDITING.contains(&chord.key)
    {
        return false;
    }
    list.commands().iter().any(|command| {
        keymap.chords(command.id).any(|own| own == chord)
    })
}

/// Keeps the window's chords from the page that has the keys.
fn hold_chords(
    webview: Res<FocusedWebview>,
    list: Res<CommandList>,
    keymap: Res<Keymap>,
    mut filter: ResMut<CefKeyboardFilter>,
) {
    let Some(page) = webview.0 else {
        filter.set([]);
        return;
    };
    let held = list
        .commands()
        .iter()
        .flat_map(|command| keymap.chords(command.id))
        .filter(|&chord| is_windows(chord, &list, &keymap))
        .map(|chord| (page, chord.key, modifiers(chord.mods)))
        .collect::<Vec<_>>();
    filter.set(held);
}

/// `mods` as the engine names them, Command as the logo key.
fn modifiers(mods: Mods) -> ModifiersState {
    ModifiersState {
        alt: mods.alt,
        ctrl: mods.ctrl,
        shift: mods.shift,
        logo: mods.primary || mods.meta,
    }
}

/// Keeps the keys typed into a page from the window, except its own
/// chords.
fn keep_typed(
    mut key: On<FocusedInput<KeyboardInput>>,
    pages: Query<(), With<Page>>,
    held: Option<Res<ButtonInput<KeyCode>>>,
    list: Res<CommandList>,
    keymap: Res<Keymap>,
) {
    let target = key.event_target();
    if target != key.original_event_target()
        || !pages.contains(target)
    {
        return;
    }
    let chord = Chord {
        key: key.input.key_code,
        mods: held.as_deref().map_or(Mods::NONE, Mods::held),
    };
    if !is_windows(chord, &list, &keymap) {
        key.propagate(false);
    }
}
