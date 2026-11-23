//! Helpers the tests of the window's parts share.

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::text::TextSpan;
use bevy::ui::widget::Text;
use bevy::ui_widgets::{Activate, Button, MenuItem};
use bevy_fynix::FynixPlugin;
use hamlet_client::connection::Contact;
use hamlet_client::outbox::Outbox;
use hamlet_client::{ClientPlugin, Connection, receive};
use hamlet_protocol::client as protocol;
/// A folder under the system's temp folder, removed on drop.
pub use hamlet_testing::Scratch;
/// The fixture state the tests of the window's parts start from.
pub use hamlet_testing::realm;
use hamlet_ui::theme::{self, HamletTheme, ThemeChanged};

use crate::browser::Browser;

/// An app with the shared panels, the designed theme and a browser
/// that fails the test if opened.
pub fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::time::TimePlugin,
        bevy::input::InputPlugin,
        bevy::input_focus::InputFocusPlugin,
        bevy::ui_widgets::MenuPlugin,
        ClientPlugin { server: None },
        FynixPlugin::<HamletTheme>::default(),
        crate::panels::plugin,
    ));
    theme::show(app.world_mut(), HamletTheme::designed());
    app.insert_resource(Browser(|_, url| {
        panic!("unexpected browser URL: {url}")
    }));
    app
}

/// The shared panels on the fixture state, with no first-run setup.
pub fn panel_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::time::TimePlugin,
        ClientPlugin { server: None },
        FynixPlugin::<HamletTheme>::default(),
        crate::panels::plugin,
    ));
    theme::show(app.world_mut(), HamletTheme::designed());
    receive(
        app.world_mut(),
        protocol::ServerMessage::State(realm::state()),
    );
    app.add_plugins((
        crate::roll::plugin,
        hamlet_ui::widgets::forms::plugin,
    ));
    app.add_message::<ThemeChanged>();
    app
}

/// Connects and receives the fixture state, its settings `unset` or
/// not.
pub fn connect(app: &mut App, unset: bool) {
    let mut state = realm::state();
    state.settings.unset = unset;
    receive(app.world_mut(), protocol::ServerMessage::State(state));
    app.world_mut().resource_mut::<Connection>().state =
        Contact::Connected;
    settle(app);
}

/// Runs the update a page follows what it received in.
pub fn settle(app: &mut App) {
    app.update();
}

/// Every text shown, one entry each.
pub fn words(app: &mut App) -> Vec<String> {
    app.world_mut()
        .query::<&Text>()
        .iter(app.world())
        .map(|text| text.0.clone())
        .collect()
}

/// Every text shown, one per line, for a search across texts.
pub fn all_words(app: &mut App) -> String {
    words(app).join("\n")
}

/// Every text and text span shown, one entry each.
pub fn words_and_spans(app: &mut App) -> Vec<String> {
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
pub fn control<T: Component>(app: &mut App, name: &str) -> Entity {
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
pub fn press(app: &mut App, text: &str) {
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
pub fn press_and_update(app: &mut App, text: &str) {
    press(app, text);
    app.update();
}

/// Takes what the window sent since last taken, as JSON.
pub fn sent(app: &mut App) -> Vec<serde_json::Value> {
    app.world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .map(|message| serde_json::to_value(message).unwrap())
        .collect()
}

/// Takes the requests sent since last taken, as JSON without `id`.
pub fn requests(app: &mut App) -> Vec<serde_json::Value> {
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
pub fn incoming(app: &mut App, value: serde_json::Value) {
    receive(app.world_mut(), serde_json::from_value(value).unwrap());
    settle(app);
}
