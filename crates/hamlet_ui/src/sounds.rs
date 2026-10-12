//! The sounds of the panels that no single button makes, each watched
//! from outside and written as a [`Cue`].

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With};
use bevy::ecs::system::{Local, Query, Res};
use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::time::Time;
use bevy_fynix::dock::{ActiveTab, DockDrag, DockTab};
use hamlet_core::session::{Reply, SessionOutput, UserMessage};
use hamlet_world::sound::{Cue, Sound};

use crate::chat::Unfolded;
use crate::hover::Card;
use crate::inspector::Section;
use crate::menu::OpenMenu;
use crate::world_menu::Open;

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

/// Writes a cue with a number of its own, so that it is not played
/// twice alike.
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

/// A menu of the top bar or a question over the world, as it opens
/// and as it goes. The menu of a thing in the world opens with the
/// sound of the hand that asked for it.
fn menus(
    bar: Res<OpenMenu>,
    over: Res<Open>,
    mut was: Local<(bool, bool, bool)>,
    mut turn: Local<u32>,
    mut cues: MessageWriter<Cue>,
) {
    let now = (
        bar.is_open(),
        matches!(*over, Open::Question(..)),
        *over != Open::Nothing,
    );
    let before = std::mem::replace(&mut *was, now);
    if now.0 && !before.0 || now.1 && !before.1 {
        write(&mut cues, &mut turn, Sound::PopOpen);
    } else if before.0 && !now.0 || before.2 && !now.2 {
        write(&mut cues, &mut turn, Sound::PopClose);
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
    section: Option<Section>,
}

/// A tab brought to the front, in the dock or in the selection, and
/// a panel taken hold of and let go.
#[allow(clippy::too_many_arguments)]
fn tabs(
    time: Res<Time>,
    drag: Res<DockDrag>,
    section: Res<Section>,
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
    // The tabs a docked panel brings with it, and the first tabs of
    // all, were not brought to the front by hand.
    let by_hand = docked.tabs && now - docked.let_go > 0.4;
    let chosen = docked.section.replace(*section);
    let turned = chosen.is_some_and(|was| was != *section);
    if !fronted.is_empty() && by_hand || turned {
        write(&mut cues, &mut turn, Sound::Tab);
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
    mut cues: MessageWriter<Cue>,
) {
    if unfolded.is_changed() && !unfolded.is_added() {
        write(&mut cues, &mut turn, Sound::Fold);
    }
    if card.shown() && !*shown {
        write(&mut cues, &mut turn, Sound::Card);
    }
    *shown = card.shown();
}

/// A message sent from the command line.
fn sent(message: On<UserMessage>, mut cues: MessageWriter<Cue>) {
    if message.key.channel == "app" {
        cues.write(cue(Sound::Send));
    }
}

/// A reply that could not be finished.
fn failed(output: On<SessionOutput>, mut cues: MessageWriter<Cue>) {
    if matches!(output.reply, Reply::Error(_)) {
        cues.write(cue(Sound::Error));
    }
}

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(sent)
        .add_observer(failed)
        .add_systems(Update, (menus, tabs, folds));
}

#[cfg(test)]
mod tests {
    use bevy::ecs::message::Messages;
    use bevy::math::Vec2;
    use hamlet_world::pick::Thing;

    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_message::<Cue>()
            .init_resource::<Time>()
            .init_resource::<OpenMenu>()
            .init_resource::<Open>()
            .init_resource::<DockDrag>()
            .init_resource::<Section>()
            .init_resource::<Unfolded>()
            .init_resource::<Card>();
        app.add_plugins(plugin);
        app.update();
        app
    }

    fn heard_in(app: &mut App) -> Vec<Sound> {
        app.update();
        let cues = app.world().resource::<Messages<Cue>>();
        cues.iter_current_update_messages()
            .map(|cue| cue.sound)
            .collect::<Vec<_>>()
    }

    #[test]
    fn a_question_over_the_world_opens_and_closes_aloud() {
        let mut app = app();
        assert_eq!(heard_in(&mut app), []);
        // The menu of a thing has the sound of the hand instead.
        *app.world_mut().resource_mut::<Open>() =
            Open::Menu(Thing::Keeper, Vec2::ZERO);
        assert_eq!(heard_in(&mut app), []);
        *app.world_mut().resource_mut::<Open>() = Open::Nothing;
        assert_eq!(heard_in(&mut app), [Sound::PopClose]);
        assert_eq!(heard_in(&mut app), []);
    }
}
