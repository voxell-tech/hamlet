//! The panels' sounds that no single button makes, as [`Cue`]s.

use bevy::app::{App, Update};
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With};
use bevy::ecs::system::{Local, Query, Res};
use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::time::Time;
use bevy_fynix::dock::{ActiveTab, DockDrag, DockTab};
use hamlet_client::messages::{ChatOutput, ChiefMessage};
use hamlet_core::sound::{Cue, Sound};
use hamlet_protocol::client as protocol;
use hamlet_ui::widgets::effects::ControlSound;

use crate::chat::ChatError;
use crate::hub::Showing;
use crate::menu::OpenMenu;
use crate::world_menu::Open;

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<ControlSound>()
        .add_observer(sent)
        .add_observer(failed)
        .add_observer(
            |_: On<ChatError>, mut cues: MessageWriter<Cue>| {
                cues.write(cue(Sound::Error));
            },
        )
        .add_systems(Update, (controls, menus, tabs));
}

/// The sounds the controls ask for.
fn controls(
    mut asked: MessageReader<ControlSound>,
    mut turn: Local<u32>,
    mut cues: MessageWriter<Cue>,
) {
    for sound in asked.read() {
        let sound = match sound {
            ControlSound::Hover => Sound::Tick,
            ControlSound::Press => Sound::Tap,
            ControlSound::Confirm => Sound::Confirm,
            ControlSound::Deny => Sound::Deny,
        };
        write(&mut cues, &mut turn, sound);
    }
}

fn cue(sound: Sound) -> Cue {
    Cue {
        sound,
        at: Vec3::ZERO,
        turn: 0,
    }
}

/// One of the window's own buttons was pressed.
pub(crate) fn window_button(world: &mut World) {
    world.write_message(cue(Sound::Sash));
}

/// Writes a cue with a number of its own.
pub(crate) fn write(
    cues: &mut MessageWriter<Cue>,
    turn: &mut u32,
    sound: Sound,
) {
    *turn = turn.wrapping_add(1);
    cues.write(Cue {
        turn: *turn,
        ..cue(sound)
    });
}

/// A top-bar menu or a question over the world, opening.
fn menus(
    bar: Res<OpenMenu>,
    over: Res<Open>,
    mut was: Local<(bool, bool)>,
    mut turn: Local<u32>,
    mut cues: MessageWriter<Cue>,
) {
    let now = (bar.is_open(), matches!(*over, Open::Question(..)));
    let before = std::mem::replace(&mut *was, now);
    if now.0 && !before.0 || now.1 && !before.1 {
        write(&mut cues, &mut turn, Sound::PopOpen);
    }
}

/// The state the dock was last seen in.
#[derive(Default)]
struct Docked {
    dragging: bool,
    /// The time a panel was last let go, in seconds.
    let_go: f32,
    /// A tab has been in front.
    tabs: bool,
}

/// A tab brought to the front, a Hub turned to another page, and a
/// panel taken and let go.
fn tabs(
    time: Res<Time>,
    drag: Res<DockDrag>,
    showing: Query<Ref<Showing>>,
    fronted: Query<(), (Added<ActiveTab>, With<DockTab>)>,
    fronts: Query<(), With<ActiveTab>>,
    mut docked: Local<Docked>,
    mut turn: Local<u32>,
    mut cues: MessageWriter<Cue>,
) {
    let now = time.elapsed_secs();
    let dragging = matches!(*drag, DockDrag::Dragging { .. });
    if dragging != docked.dragging {
        docked.dragging = dragging;
        let sound = if dragging { Sound::Grab } else { Sound::Dock };
        write(&mut cues, &mut turn, sound);
        docked.let_go = now;
    }
    // A docked panel's tabs and the first tabs were not chosen by
    // hand.
    let by_hand = docked.tabs && now - docked.let_go > 0.4;
    let turned = showing
        .iter()
        .any(|showing| showing.is_changed() && !showing.is_added());
    if !fronted.is_empty() && by_hand || turned {
        write(&mut cues, &mut turn, Sound::Tab);
    }
    docked.tabs = !fronts.is_empty();
}

/// A message sent from the command line.
fn sent(_: On<ChiefMessage>, mut cues: MessageWriter<Cue>) {
    cues.write(cue(Sound::Send));
}

/// A reply that could not be finished.
fn failed(output: On<ChatOutput>, mut cues: MessageWriter<Cue>) {
    if matches!(&output.operation, protocol::TranscriptOperation::Add { item, .. } if item.kind == protocol::ItemKind::Error)
    {
        cues.write(cue(Sound::Error));
    }
}
