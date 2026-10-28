//! Helpers the tests of several modules share.

use std::io::Cursor;

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::math::UVec2;
use bevy::text::TextSpan;
use bevy::ui::widget::Text;
use bevy::ui_widgets::{Activate, Button, MenuItem};
use bevy_fynix::FynixPlugin;
use bevy_fynix::dock::DockTree;
use hamlet_assets::Tuning;
use hamlet_client::connection::Contact;
use hamlet_client::outbox::Outbox;
use hamlet_client::{ClientPlugin, Connection, receive};
use hamlet_desktop::awake::KeepAwake;
use hamlet_protocol::client as protocol;
use image::{ImageFormat, RgbaImage};

use crate::markdown::{Block, Kind, Span, Style};
use crate::screens::settings::{self, Pane};
use crate::theme::{self, HamletTheme, ThemeChanged};
use crate::welcome::Browser;

/// A window's panels on the received fixture state: the chat's
/// agent, the roll and the asking, with no page of first-run setup.
pub(crate) fn panel_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::time::TimePlugin,
        ClientPlugin { server: None },
        FynixPlugin::<HamletTheme>::default(),
        crate::panel_sets,
    ));
    theme::insert_theme(app.world_mut(), HamletTheme::designed());
    receive(
        app.world_mut(),
        protocol::ServerMessage::State(hamlet_testing::realm::state()),
    );
    app.add_plugins((
        crate::convo::plugin,
        crate::roll::plugin,
        crate::asking::plugin,
    ));
    app.init_resource::<DockTree>()
        .init_resource::<crate::select::Selecting>()
        .add_message::<ThemeChanged>();
    app
}

/// First-run setup with input and the secret prompt, and a browser
/// that fails the test if anything opens it. Nothing is received
/// until [`connect`].
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
        crate::secret::plugin,
        crate::panel_sets,
    ));
    theme::insert_theme(app.world_mut(), HamletTheme::designed());
    app.insert_resource(Browser(|_, url| {
        panic!("unexpected browser URL: {url}")
    }));
    app
}

/// Connects and receives the fixture state, its settings unset or
/// not, and runs the update the pages follow it in.
pub(crate) fn connect(app: &mut App, unset: bool) {
    let mut state = hamlet_testing::realm::state();
    state.settings.unset = unset;
    receive(app.world_mut(), protocol::ServerMessage::State(state));
    app.world_mut().resource_mut::<Connection>().state =
        Contact::Connected;
    settle(app);
}

/// The settings screen open on `pane`, over [`panel_app`].
pub(crate) fn settings_page(pane: Pane) -> App {
    let mut app = panel_app();
    app.add_plugins((settings::plugin, crate::secret::plugin))
        .init_resource::<KeepAwake>()
        .insert_resource(pane);
    bevy_fynix::mount::<HamletTheme>(
        app.world_mut(),
        settings::screen(),
    );
    settle(&mut app);
    app
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

/// The control of kind `T` beside the label that says `name`: the
/// first found under the label's parent.
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

/// Activates the first button or menu item whose label says `text`,
/// with no frame run after.
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

/// Takes what the window has sent the server since it was last
/// taken, as JSON.
pub(crate) fn sent(app: &mut App) -> Vec<serde_json::Value> {
    app.world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .map(|message| serde_json::to_value(message).unwrap())
        .collect()
}

/// Takes the requests the window has sent since they were last
/// taken, as JSON with no `id`, after checking each has one; chats
/// it opened are left out.
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

/// Receives the server message `value` and runs the update the
/// pages follow it in.
pub(crate) fn incoming(app: &mut App, value: serde_json::Value) {
    receive(app.world_mut(), serde_json::from_value(value).unwrap());
    settle(app);
}

/// One span of `text` in no style and with no link.
pub(crate) fn plain(text: &str) -> Span {
    Span {
        text: text.into(),
        style: Style::default(),
        link: None,
    }
}

/// A block of `kind` outside any quote or list.
pub(crate) fn top(kind: Kind) -> Block {
    Block {
        quote: 0,
        indent: 0,
        marker: None,
        kind,
    }
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
