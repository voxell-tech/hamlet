//! The sounds of the panels that no single button makes, each watched
//! from outside and written as a [`PanelCue`].

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With};
use bevy::ecs::system::{Local, Query, Res};
use bevy::ecs::world::World;
use bevy::time::Time;
use bevy_fynix::dock::{ActiveTab, DockDrag, DockTab};
use hamlet_core::session::{Reply, SessionOutput, UserMessage};
use hamlet_realm_api::sound::{PanelCue, PanelSound, write};

use crate::chat::Unfolded;
use crate::hover::Card;

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<PanelCue>()
        .add_observer(sent)
        .add_observer(failed)
        .add_systems(Update, (tabs, folds));
}

fn cue(sound: PanelSound) -> PanelCue {
    PanelCue { sound, turn: 0 }
}

/// One of the window's own buttons was pressed.
pub(crate) fn window_button(world: &mut World) {
    world.write_message(cue(PanelSound::Sash));
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

/// A tab brought to the front in the dock, and a panel taken hold of
/// and let go.
fn tabs(
    time: Res<Time>,
    drag: Res<DockDrag>,
    fronted: Query<(), (Added<ActiveTab>, With<DockTab>)>,
    fronts: Query<(), With<ActiveTab>>,
    mut docked: Local<Docked>,
    mut turn: Local<u32>,
    mut cues: MessageWriter<PanelCue>,
) {
    let now = time.elapsed_secs();
    let dragging = matches!(*drag, DockDrag::Dragging { .. });
    if dragging != docked.dragging {
        docked.dragging = dragging;
        let sound = if dragging {
            PanelSound::Grab
        } else {
            PanelSound::Dock
        };
        write(&mut cues, &mut turn, sound);
        docked.let_go = now;
    }
    // The tabs a docked panel brings with it, and the first tabs of
    // all, were not brought to the front by hand.
    let by_hand = docked.tabs && now - docked.let_go > 0.4;
    if !fronted.is_empty() && by_hand {
        write(&mut cues, &mut turn, PanelSound::Tab);
    }
    docked.tabs = !fronts.is_empty();
}

/// A fold of the conversation opened or shut, and the card of a
/// thing the pointer rests on as it appears.
fn folds(
    unfolded: Res<Unfolded>,
    card: Res<Card>,
    mut shown: Local<bool>,
    mut turn: Local<u32>,
    mut cues: MessageWriter<PanelCue>,
) {
    if unfolded.is_changed() && !unfolded.is_added() {
        write(&mut cues, &mut turn, PanelSound::Fold);
    }
    if card.shown() && !*shown {
        write(&mut cues, &mut turn, PanelSound::Card);
    }
    *shown = card.shown();
}

/// A message sent from the command line.
fn sent(message: On<UserMessage>, mut cues: MessageWriter<PanelCue>) {
    if message.key.channel == "app" {
        cues.write(cue(PanelSound::Send));
    }
}

/// A reply that could not be finished.
fn failed(
    output: On<SessionOutput>,
    mut cues: MessageWriter<PanelCue>,
) {
    if matches!(output.reply, Reply::Error(_)) {
        cues.write(cue(PanelSound::Error));
    }
}
