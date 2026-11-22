//! Helpers the tests of several modules share.

use std::io::Cursor;
use std::path::PathBuf;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::math::UVec2;
use bevy::text::TextSpan;
use bevy::ui::widget::Text;
use bevy::ui_widgets::{Activate, Button, MenuItem};
use bevy_fynix::FynixPlugin;
use bevy_fynix::dock::{DockRegistry, DockTree};
use hamlet_client::connection::Contact;
use hamlet_client::outbox::Outbox;
use hamlet_client::{ClientPlugin, Connection, ViewFolder, receive};
use hamlet_core::browser::Browser;
use hamlet_core::pick::Selected;
use hamlet_core::roll::Roll;
use hamlet_core::settings::SettingsPane;
use hamlet_desktop::awake::KeepAwake;
use hamlet_protocol::client as protocol;
use hamlet_realm::North;
use hamlet_realm::sky::{
    DistanceFogSettings, Quality, VolumetricFogSettings,
};
use hamlet_realm::sound::SoundSettings;
use hamlet_ui::theme::{self, HamletTheme, ThemeChanged};
use hamlet_vfx::wind::WindChoice;
use image::{ImageFormat, RgbaImage};

use crate::appearance::Appearance;
use crate::desk::{self, Desks};
use crate::remember::Remembered;
use crate::screens::settings;
use crate::world_menu::Favorites;
use crate::world_view::{NorthTarget, TimeOfDay};

/// A window's panels on the fixture state, with no first-run setup.
pub(crate) fn panel_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::time::TimePlugin,
        ClientPlugin { server: None },
        FynixPlugin::<HamletTheme>::default(),
        hamlet_core::panels::plugin,
    ));
    theme::show(app.world_mut(), HamletTheme::designed());
    receive(
        app.world_mut(),
        protocol::ServerMessage::State(hamlet_testing::realm::state()),
    );
    app.add_plugins((
        hamlet_core::roll::plugin,
        crate::asking::plugin,
        hamlet_ui::widgets::forms::plugin,
    ));
    app.init_resource::<DockTree>()
        .init_resource::<crate::select::Selecting>()
        .add_message::<ThemeChanged>();
    hub(&mut app);
    app
}

/// The first Hub in a dock of its own, following the world's
/// selection.
pub(crate) fn hub(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.get_resource_or_init::<DockTree>().set_root_leaf(
        bevy_fynix::dock::DockLeaf::new(
            "side",
            bevy_fynix::dock::DockAreaStyle::TabBar,
        )
        .with_windows(vec![crate::dock::HUB.into()]),
    );
    crate::hub::first(world)
}

/// The chat of the first Hub.
pub(crate) fn convo(app: &App) -> &crate::convo::Convo {
    let hub = crate::hub::following(app.world()).unwrap();
    app.world().get::<crate::convo::Convo>(hub).unwrap()
}

pub(crate) fn seat_hand(app: &mut App) -> Entity {
    let hand =
        hamlet_client::realm::named(app.world_mut(), "hand").unwrap();
    let hub = hub(app);
    crate::convo::seat(app.world_mut(), hub, hand);
    hand
}

pub(crate) fn seat_ada(app: &mut App) -> Entity {
    let ada = hamlet_client::realm::named(app.world_mut(), "agent#2")
        .unwrap();
    let hub = hub(app);
    crate::convo::seat(app.world_mut(), hub, ada);
    ada
}

/// First-run setup with a browser that fails the test if opened.
pub(crate) fn welcome_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::time::TimePlugin,
        bevy::input::InputPlugin,
        bevy::input_focus::InputFocusPlugin,
        bevy::ui_widgets::MenuPlugin,
        ClientPlugin { server: None },
        FynixPlugin::<HamletTheme>::default(),
        crate::welcome::plugin,
        hamlet_ui::widgets::secret::plugin,
        hamlet_core::panels::plugin,
    ));
    theme::show(app.world_mut(), HamletTheme::designed());
    app.insert_resource(Browser(|_, url| {
        panic!("unexpected browser URL: {url}")
    }));
    app
}

/// Connects and receives the fixture state, its settings `unset` or
/// not.
pub(crate) fn connect(app: &mut App, unset: bool) {
    let mut state = hamlet_testing::realm::state();
    state.settings.unset = unset;
    receive(app.world_mut(), protocol::ServerMessage::State(state));
    app.world_mut().resource_mut::<Connection>().state =
        Contact::Connected;
    settle(app);
}

/// The settings screen open on `pane`, over [`panel_app`].
pub(crate) fn settings_page(pane: Option<&'static str>) -> App {
    let mut app = panel_app();
    app.add_plugins((
        settings::plugin,
        hamlet_ui::widgets::secret::plugin,
    ))
    .init_resource::<KeepAwake>()
    .insert_resource(SettingsPane(pane));
    bevy_fynix::mount::<HamletTheme>(
        app.world_mut(),
        settings::screen(),
    );
    settle(&mut app);
    app
}

/// Writes the view into `folder` as the window does.
pub(crate) fn keeping(app: &mut App, folder: PathBuf) {
    app.insert_resource(ViewFolder(folder))
        .init_resource::<Selected>()
        .init_resource::<Roll>()
        .init_resource::<DockTree>()
        .init_resource::<TimeOfDay>()
        .init_resource::<SoundSettings>()
        .init_resource::<Quality>()
        .init_resource::<VolumetricFogSettings>()
        .init_resource::<DistanceFogSettings>()
        .init_resource::<WindChoice>()
        .init_resource::<North>()
        .init_resource::<NorthTarget>()
        .init_resource::<KeepAwake>()
        .init_resource::<DockRegistry<HamletTheme>>()
        .init_resource::<Desks>()
        .init_resource::<Favorites>()
        .init_resource::<Appearance>()
        .init_resource::<Remembered>()
        .init_resource::<desk::Shown>()
        .init_resource::<crate::screens::terminal::Shown>()
        .init_resource::<crate::screens::terminal::Folds>()
        .init_resource::<crate::screens::terminal::Pins>()
        .init_resource::<crate::hub::CollapsedRails>()
        .init_resource::<crate::updates::UpdateChannel>()
        .add_systems(Update, desk::capture)
        .add_plugins(crate::remember::plugin);
}

/// Runs the update a page follows what it received in.
pub(crate) fn settle(app: &mut App) {
    app.update();
}

/// Every text shown, one entry each.
pub(crate) fn words(app: &mut App) -> Vec<String> {
    app.world_mut()
        .query::<&Text>()
        .iter(app.world())
        .map(|text| text.0.clone())
        .collect()
}

/// Every text shown, one per line, for a search across texts.
pub(crate) fn all_words(app: &mut App) -> String {
    words(app).join("\n")
}

/// Every text and text span shown, one entry each.
pub(crate) fn words_and_spans(app: &mut App) -> Vec<String> {
    let mut shown = words(app);
    shown.extend(
        app.world_mut()
            .query::<&TextSpan>()
            .iter(app.world())
            .map(|span| span.0.clone()),
    );
    shown
}

/// The control of kind `T` beside the label `name`.
pub(crate) fn control<T: Component>(
    app: &mut App,
    name: &str,
) -> Entity {
    let world = app.world_mut();
    let label = world
        .query::<(Entity, &Text)>()
        .iter(world)
        .find(|(_, text)| text.0 == name)
        .unwrap_or_else(|| panic!("no field says {name}"))
        .0;
    let mut pending =
        vec![world.get::<ChildOf>(label).unwrap().parent()];
    while let Some(entity) = pending.pop() {
        if world.get::<T>(entity).is_some() {
            return entity;
        }
        if let Some(children) = world.get::<Children>(entity) {
            pending.extend(children.iter());
        }
    }
    panic!("no control under {name}");
}

/// Activates the first button or menu item labelled `text`.
pub(crate) fn press(app: &mut App, text: &str) {
    let world = app.world_mut();
    let entity = world
        .query::<(Entity, &Text)>()
        .iter(world)
        .filter(|(_, shown)| shown.0 == text)
        .find_map(|(mut entity, _)| {
            loop {
                if world.get::<Button>(entity).is_some()
                    || world.get::<MenuItem>(entity).is_some()
                {
                    return Some(entity);
                }
                entity = world.get::<ChildOf>(entity)?.parent();
            }
        })
        .unwrap_or_else(|| panic!("no button says {text}"));
    world.trigger(Activate { entity });
    world.flush();
}

/// [`press`], then the update a page answers it in.
pub(crate) fn press_and_update(app: &mut App, text: &str) {
    press(app, text);
    app.update();
}

/// Takes what the window sent since last taken, as JSON.
pub(crate) fn sent(app: &mut App) -> Vec<serde_json::Value> {
    app.world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .map(|message| serde_json::to_value(message).unwrap())
        .collect()
}

/// Takes the requests sent since last taken, as JSON without `id`.
pub(crate) fn requests(app: &mut App) -> Vec<serde_json::Value> {
    app.world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .map(|message| serde_json::to_value(message).unwrap())
        .filter(|message| message["type"] != "open")
        .map(|mut message| {
            assert!(
                message["id"]
                    .as_str()
                    .is_some_and(|id| id.len() == 36)
            );
            message.as_object_mut().unwrap().remove("id");
            message
        })
        .collect()
}

/// Receives the server message `value` and settles.
pub(crate) fn incoming(app: &mut App, value: serde_json::Value) {
    receive(app.world_mut(), serde_json::from_value(value).unwrap());
    settle(app);
}

/// A PNG of `size`, one colour all over.
pub(crate) fn png(size: UVec2) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    RgbaImage::from_pixel(
        size.x,
        size.y,
        image::Rgba([200, 150, 60, 255]),
    )
    .write_to(&mut bytes, ImageFormat::Png)
    .unwrap();
    bytes.into_inner()
}
