//! The menus: HAMLET and VIEW in the top bar, the menu a right click
//! opens on a thing of the world with the window's own items, the
//! form a hamlet is founded with and the question asked before a
//! deed.

pub mod menu;
pub mod world_menu;

use bevy::app::{App, Update};
use bevy::ecs::message::MessageWriter;
use bevy::ecs::system::{Local, Res};
use bevy_fynix::ViewExt;
use hamlet_chat::{chat, me};
use hamlet_realm_api::sound::{PanelCue, PanelSound, write};
use hamlet_sdk::frame::Place;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};
use hamlet_sdk::{AnyView, Cx, View, feel, icons, looks, platform};

use self::menu::{Menu, OpenMenu};
use self::world_menu::Open;

/// Adds the Menus plugin.
pub fn plugin(app: &mut App) {
    app.add_plugins((menu::plugin, world_menu::plugin))
        .add_message::<PanelCue>()
        .add_systems(Update, sound)
        .add_hamlet_plugin::<MenusPlugin>();
}

/// The plugin that draws the menus and owns the items of the menu of
/// a thing in the world. The chief cannot lose it: the menus are the
/// way to quit.
pub struct MenusPlugin;

impl HamletPlugin for MenusPlugin {
    fn manifest() -> Manifest {
        Manifest::built_in("menus").named("Menus").essential()
    }

    fn entry(entry: &mut Entry) {
        world_menu::contribute(entry);
        entry.command("found", "Found a Hamlet...", |commands| {
            commands.queue(world_menu::ask_found);
        });
        entry.menu_item(
            Menu::App,
            0,
            "Found a Hamlet...",
            "found",
            Some(Glyph::Plus),
        );
        entry.frame_part(Place::Bar, 0, || menu::titles().boxed());
        entry
            .frame_part(Place::Over, 0, || menu::open_menu().boxed());
        entry.frame_part(Place::Over, 10, world_menu::view);
    }
}

/// A menu of the top bar or a question over the world, as it opens
/// and as it goes. The menu of a thing in the world opens with the
/// sound of the hand that asked for it.
fn sound(
    bar: Res<OpenMenu>,
    over: Res<Open>,
    mut was: Local<(bool, bool, bool)>,
    mut turn: Local<u32>,
    mut cues: MessageWriter<PanelCue>,
) {
    let now = (
        bar.is_open(),
        matches!(*over, Open::Question(..)),
        *over != Open::Nothing,
    );
    let before = std::mem::replace(&mut *was, now);
    if now.0 && !before.0 || now.1 && !before.1 {
        write(&mut cues, &mut turn, PanelSound::PopOpen);
    } else if before.0 && !now.0 || before.2 && !now.2 {
        write(&mut cues, &mut turn, PanelSound::PopClose);
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::message::Messages;
    use bevy::math::Vec2;
    use hamlet_realm_api::pick::Thing;

    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_message::<PanelCue>()
            .init_resource::<OpenMenu>()
            .init_resource::<Open>()
            .add_systems(Update, sound);
        app.update();
        app
    }

    fn heard_in(app: &mut App) -> Vec<PanelSound> {
        app.update();
        let cues = app.world().resource::<Messages<PanelCue>>();
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
        assert_eq!(heard_in(&mut app), [PanelSound::PopClose]);
        assert_eq!(heard_in(&mut app), []);
    }
}
