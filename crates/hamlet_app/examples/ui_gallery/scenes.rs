//! Scenes of the panels and their controls held in fixed states.

mod dock;
mod sample;
mod sheets;

use bevy::app::{
    App, Plugin, PostStartup, PostUpdate, Startup, Update,
};
use bevy::asset::Handle;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Res;
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::math::Rect;
use bevy::picking::PickingSettings;
use bevy_fynix::dock::DockPlugin;
use bevy_fynix::tokens::Tone;
use bevy_fynix::{AnyView, Bevy, FynixPlugin, mount};
use hamlet_app::history::History;
use hamlet_app::{Bar, Roll, asking, chrome, menu, select, welcome};
use hamlet_realm::sound::{Cue, SoundSettings};
use hamlet_ui::theme::{self, HamletTheme};

use self::dock::{open_settings, panels, seed_dock};
use self::sample::{first_run, setup};
use self::sheets::{controls, hold, hold_message, icon_sheet};

/// A view of the gallery, its type erased.
type Ui = AnyView<Bevy, HamletTheme>;

/// Draws one [`Scene`] with the panels' own views and looks.
pub struct GalleryPlugin(pub Scene);

impl Plugin for GalleryPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            hamlet_client::ClientPlugin { server: None },
            FynixPlugin::<HamletTheme>::default(),
            DockPlugin::<HamletTheme>::default(),
        ));
        theme::show(app.world_mut(), HamletTheme::designed());
        app.insert_resource(self.0)
            // No pointer moves here, so a held hover stays held.
            .insert_resource(PickingSettings {
                is_hover_enabled: false,
                ..PickingSettings::default()
            })
            .insert_resource(Bar {
                running: "Running, idle".into(),
                running_tone: Tone::Dim,
                model: "hearth-large".into(),
                link: "Connected, 1 model".into(),
                usage: "12.4k tokens".into(),
                budget: "$1.20 of $20.00".into(),
                ..Bar::default()
            })
            .init_resource::<Roll>()
            .init_resource::<SoundSettings>()
            .init_resource::<History>()
            .init_resource::<hamlet_app::limits::Book>()
            .init_resource::<hamlet_app::models::Shelf>()
            .init_resource::<hamlet_app::models::Draft>()
            .init_resource::<hamlet_app::limits::Part>()
            .init_resource::<menu::OpenMenu>()
            .insert_resource(first_run(self.0))
            .init_resource::<chrome::ChromeSettings>()
            .add_message::<Cue>()
            // `theme::plugin` owns it; the gallery sets its theme by
            // hand instead.
            .add_message::<theme::ThemeChanged>()
            .add_systems(Startup, (setup, chrome::draw_marks))
            .add_systems(
                PostStartup,
                (seed_dock, mount_scene).chain(),
            )
            .add_systems(PostUpdate, open_settings)
            .add_systems(
                Update,
                (
                    sample::overview::later,
                    hold,
                    hold_message.run_if(|scene: Res<Scene>| {
                        *scene == Scene::Rewind
                    }),
                ),
            )
            // The menus, and what the world tells them was asked
            // about.
            .init_resource::<hamlet_realm::pick::Asked>()
            .init_resource::<hamlet_realm::pick::AskedGround>()
            .add_plugins((
                hamlet_app::world_menu::plugin,
                hamlet_app::search::plugin,
                hamlet_app::desk::plugin,
                hamlet_app::toasts::plugin,
                hamlet_app::secret::plugin,
                hamlet_ui::widgets::plugin,
                hamlet_app::screens::plugin,
                asking::plugin,
                select::plugin,
            ));
    }
}

/// The scene a gallery shows.
#[derive(Resource, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    /// Every control in each of its states.
    Controls,
    /// The three panels at rest.
    Panels,
    /// The panels while something waits for the chief.
    Waiting,
    /// The panels while the chat's field is typed in.
    Field,
    /// A long chat with a turn's steps unfolded.
    Chat,
    /// An agent's chat with the chief, the mayor and another agent
    /// in it.
    Group,
    /// A chat with links of every kind in it, the pointer on one.
    Links,
    /// A chat with the changes of a turn's edits in it.
    Edits,
    /// A chat with pictures in it: drawn, made smaller, missing and
    /// one of the web that waits to be asked for.
    Pictures,
    /// A chat with the pointer on one of the chief's messages, and
    /// the question its mark asks before the chat is taken back to
    /// it.
    Rewind,
    /// A chat nothing has been said in.
    Quiet,
    /// An agent's memory.
    Memory,
    /// An agent's character.
    Character,
    /// The whole of an agent's prompt.
    Prompt,
    /// The Hub with its rail open over a realm at work.
    Hub,
    /// The Hub's page of a scholar.
    Scholar,
    /// The Hub's page of the realm, over a realm at work.
    Realm,
    /// The starting desk, narrow enough for the rail's icons.
    Desk,
    /// Toasts stacked over the realm.
    Toasts,
    /// The popup an agent's model is chosen in, over the Hub.
    Models,
    /// That popup with a search typed in it, found across every
    /// provider.
    ModelsSearch,
    /// The Hub's page of the realm, with nothing selected.
    Overview,
    /// One hamlet and everyone in it.
    Hamlet,
    /// The running of that hamlet: its budget, models and limits.
    Bylaws,
    /// The Hub's sessions page, with its list of sessions.
    History,
    /// The Hub's sessions page, with one session being read.
    Reading,
    /// A chief, with every choice of its look.
    Chief,
    /// The Settings panel.
    Settings,
    /// The Settings panel at its Models page.
    SettingsModels,
    /// The Settings panel at its Charter page.
    Charter,
    /// The Settings panel at its Workspace page, a remote workspace
    /// chosen and not yet in use.
    Workspace,
    /// The Settings panel at its Sight page, with its dials.
    Sight,
    /// Every icon, at each size it is drawn for.
    Icons,
    /// The panels with the HAMLET menu open.
    Menu,
    /// The first-run screen at one of its steps.
    Welcome(welcome::Step),
}

/// The picture that stands where the world is drawn, and the part of
/// it shown.
#[derive(Resource)]
pub struct Stand(pub Handle<Image>, pub Rect);

/// The update on which the buttons shown as done are pressed.
#[derive(Resource)]
pub struct Strike(pub u32);

fn mount_scene(world: &mut World) {
    let view = match *world.resource::<Scene>() {
        Scene::Controls => controls(),
        Scene::Icons => icon_sheet(),
        _ => panels(),
    };
    mount::<HamletTheme>(world, view);
    // The menus and questions the app's own root draws over all.
    mount::<HamletTheme>(world, hamlet_app::world_menu::view());
}
