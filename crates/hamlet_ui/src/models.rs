//! The server's catalogue and each hamlet's choices of models.

mod choice;
mod home;

use std::collections::BTreeMap;

use bevy::app::{App, Update};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, ResMut};
use bevy::ecs::world::World;
use hamlet_client::Connection;
use hamlet_client::connection::State;
use hamlet_client::messages::{Reconnected, Response};
use hamlet_client::outbox::Outbox;
use hamlet_protocol::client as protocol;

pub(crate) use self::choice::{of_agent, of_hamlet};
pub(crate) use self::home::narrowed;
pub use self::home::{endpoints, shelf};

const SHOWN: usize = 40;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Shelf>()
        .init_resource::<Draft>()
        .add_observer(answered)
        .add_observer(
            |_: On<Reconnected>, mut shelf: ResMut<Shelf>| {
                shelf.loaded = false;
                shelf.asking = false;
            },
        )
        .add_systems(Update, follow);
}

/// The catalogue last received from the server.
#[derive(Resource, Default)]
pub struct Shelf {
    pub models: Vec<protocol::Listed>,
    asking: bool,
    loaded: bool,
    showing: bool,
    problem: Option<String>,
    requests: BTreeMap<String, Asked>,
}

#[derive(Clone, Copy)]
enum Asked {
    Models,
    Edit,
    Default,
}

/// The catalogue's search field.
#[derive(Resource, Default)]
pub struct Draft {
    narrow: String,
}

pub fn follow(world: &mut World) {
    let showing = world.get_resource::<crate::settings::Pane>()
        == Some(&crate::settings::Pane::Models);
    let shelf = world.resource::<Shelf>();
    let read =
        !shelf.asking && (!shelf.loaded || showing && !shelf.showing);
    if shelf.showing != showing {
        world.resource_mut::<Shelf>().showing = showing;
    }
    if read
        && world.get_resource::<Connection>().is_some_and(
            |connection| connection.state == State::Connected,
        )
    {
        read_models(world, false);
    }
}

fn read_models(world: &mut World, refresh: bool) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Read(protocol::Read::Models {
            id,
            refresh: Some(refresh),
        })
    });
    let mut shelf = world.resource_mut::<Shelf>();
    shelf
        .requests
        .retain(|_, asked| !matches!(asked, Asked::Models));
    shelf.requests.insert(id, Asked::Models);
    shelf.asking = true;
    shelf.problem = None;
}

fn edit(
    world: &mut World,
    make: impl FnOnce(String) -> protocol::Edit,
) {
    let id = world
        .resource_mut::<Outbox>()
        .send(|id| protocol::ClientMessage::Edit(make(id)));
    let mut shelf = world.resource_mut::<Shelf>();
    shelf.requests.insert(id, Asked::Edit);
    shelf.problem = None;
}

fn problem() -> impl crate::View {
    crate::looks::note(
        bevy_fynix::derived(|world: &World| {
            world
                .get_resource::<Shelf>()
                .and_then(|shelf| shelf.problem.clone())
                .unwrap_or_default()
        })
        .when(crate::roll::moved::<Shelf>()),
    )
}

fn catalogue(
    world: &mut World,
    model: &str,
    set: protocol::EditCatalogueSet,
) {
    edit(world, |id| protocol::Edit::Catalogue {
        id,
        model: model.into(),
        set,
    });
}

fn answered(response: On<Response>, mut commands: Commands) {
    let response = response.event().clone();
    commands.queue(move |world: &mut World| {
        let Some(asked) = world
            .resource_mut::<Shelf>()
            .requests
            .remove(&response.to)
        else {
            return;
        };
        let failure = match &response.message {
            protocol::ServerMessage::Fault(fault) => {
                Some(fault.text.clone())
            }
            protocol::ServerMessage::Reply(reply)
                if reply.kind == protocol::ReplyKind::Error =>
            {
                Some(reply.text.clone())
            }
            _ => None,
        };
        if let Some(problem) = failure {
            let mut shelf = world.resource_mut::<Shelf>();
            shelf.problem = Some(problem);
            if matches!(asked, Asked::Models) {
                shelf.asking = false;
                shelf.loaded = true;
            }
            return;
        }
        match (asked, response.message) {
            (
                Asked::Models,
                protocol::ServerMessage::Read(
                    protocol::ReadResult::Models { models, .. },
                ),
            ) => {
                let mut shelf = world.resource_mut::<Shelf>();
                shelf.models = models;
                shelf.asking = false;
                shelf.loaded = true;
            }
            (Asked::Edit, protocol::ServerMessage::Reply(_)) => {
                read_models(world, false)
            }
            (Asked::Default, protocol::ServerMessage::Reply(_)) => {
                world.resource_mut::<Outbox>().send(|id| {
                    protocol::ClientMessage::Check(
                        protocol::Check::Model { id, model: None },
                    )
                });
            }
            _ => {}
        }
    });
}
#[cfg(test)]
mod tests {
    use bevy_fynix::mount;
    use serde_json::json;

    use super::*;
    use crate::theme::HamletTheme;
    use crate::welcome::server_tests::{
        app, incoming, sent, show, words,
    };

    #[test]
    fn the_models_page_reads_the_catalogue_and_shows_the_servers_prices_and_notes()
     {
        let mut app = app();
        app.add_plugins(plugin);
        show(&mut app, false);
        let requests = sent(&mut app);
        assert_eq!(
            requests.len(),
            1,
            "a catalogue read: {requests:?}"
        );
        assert_eq!(requests[0]["what"], "models");
        incoming(
            &mut app,
            json!({"type":"read", "to":requests[0]["id"], "models":[
                {"name":"far::large", "price":{"input":3.0,"output":15.0}, "priced":"listed", "context":200000, "allowed":true, "note":"For hard questions."}
            ]}),
        );
        mount::<HamletTheme>(app.world_mut(), shelf());
        let shown = words(&mut app);
        assert!(
            app.world_mut()
                .query::<&bevy::text::EditableText>()
                .iter(app.world())
                .any(|text| text.value() == "For hard questions.")
        );
        for text in ["far::large", "3.00", "15.00", "listed", "200k"]
        {
            assert!(shown.contains(text), "{shown}");
        }
    }
    fn with_catalogue() -> bevy::app::App {
        let mut app = app();
        app.add_plugins((plugin, crate::roll::plugin));
        show(&mut app, false);
        let read = sent(&mut app).remove(0);
        incoming(
            &mut app,
            json!({"type":"read", "to":read["id"], "models":[
                {"name":"far::small", "price":null, "priced":"unknown", "context":null, "allowed":true, "note":""}
            ]}),
        );
        app
    }

    #[test]
    fn allowance_refresh_and_a_refused_catalogue_edit_go_through_the_server()
     {
        use bevy::ecs::entity::Entity;
        use bevy::ecs::query::With;
        use bevy::ui_widgets::{Checkbox, ValueChange};

        use crate::welcome::server_tests::press;
        let mut app = with_catalogue();
        mount::<HamletTheme>(app.world_mut(), shelf());
        let toggle = app
            .world_mut()
            .query_filtered::<Entity, With<Checkbox>>()
            .single(app.world())
            .unwrap();
        app.world_mut().trigger(ValueChange {
            source: toggle,
            value: false,
            is_final: true,
        });
        app.update();
        let edit = sent(&mut app).remove(0);
        assert_eq!(edit["doc"], "catalogue");
        assert_eq!(edit["model"], "far::small");
        assert_eq!(edit["set"], json!({"allowed":false}));
        incoming(
            &mut app,
            json!({"type":"reply", "to":edit["id"], "kind":"error", "text":"The catalogue could not be saved."}),
        );
        assert!(
            words(&mut app)
                .contains("The catalogue could not be saved.")
        );
        press(&mut app, "LIST MODELS");
        let read = sent(&mut app).remove(0);
        assert_eq!(read["what"], "models");
        assert_eq!(read["refresh"], true);
    }

    #[test]
    fn the_mayors_model_is_pinned_by_its_office() {
        use crate::welcome::server_tests::press;
        let mut app = with_catalogue();
        let founder =
            hamlet_client::realm::mayor(app.world_mut()).unwrap();
        mount::<HamletTheme>(app.world_mut(), of_agent(founder));
        press(&mut app, "faux::faux-1");
        press(&mut app, "far::small");
        let edit = sent(&mut app).remove(0);
        assert_eq!(edit["doc"], "limits");
        assert_eq!(edit["hamlet"], "home");
        assert_eq!(
            edit["set"],
            json!({"section":"pinned", "key":"mayor", "value":"far::small"})
        );
        incoming(
            &mut app,
            json!({"type":"reply", "to":edit["id"], "kind":"error", "text":"This pin was refused."}),
        );
        assert!(words(&mut app).contains("This pin was refused."));
    }

    #[test]
    fn an_agents_model_is_pinned_by_its_name() {
        use crate::welcome::server_tests::press;
        let mut app = with_catalogue();
        incoming(
            &mut app,
            json!({"type":"change", "epoch":1, "revision":1,
            "changes":[{"type":"agent", "agent":hamlet_testing::realm::agent("home", 1, "scout")}]}),
        );
        let scout =
            hamlet_client::realm::named(app.world_mut(), "scout")
                .unwrap();
        mount::<HamletTheme>(app.world_mut(), of_agent(scout));
        press(&mut app, "faux::faux-1");
        press(&mut app, "far::small");
        let edit = sent(&mut app).remove(0);
        assert_eq!(
            edit["set"],
            json!({"section":"pinned", "key":"scout", "value":"far::small"})
        );
    }

    #[test]
    fn a_hamlets_list_and_default_keep_the_rest_of_its_received_limits()
     {
        use bevy::ecs::entity::Entity;
        use bevy::ecs::query::With;
        use bevy::ui_widgets::{Checkbox, ValueChange};

        use crate::welcome::server_tests::press;
        let mut app = with_catalogue();
        let founder =
            hamlet_client::realm::mayor(app.world_mut()).unwrap();
        let hamlet = app
            .world()
            .get::<hamlet_client::realm::LivesIn>(founder)
            .unwrap()
            .0;
        mount::<HamletTheme>(app.world_mut(), of_hamlet(hamlet));
        let toggle = app
            .world_mut()
            .query_filtered::<Entity, With<Checkbox>>()
            .single(app.world())
            .unwrap();
        app.world_mut().trigger(ValueChange {
            source: toggle,
            value: true,
            is_final: true,
        });
        app.update();
        let edit = sent(&mut app).remove(0);
        assert_eq!(edit["whole"]["models"], json!(["far::small"]));
        assert_eq!(edit["whole"]["mostAgents"], 12);
        assert_eq!(edit["whole"]["budget"]["cost"], 5.0);
        incoming(
            &mut app,
            json!({"type":"reply", "to":edit["id"], "kind":"error", "text":"This model list was refused."}),
        );
        assert!(
            words(&mut app).contains("This model list was refused.")
        );
        press(&mut app, "faux::faux-1");
        press(&mut app, "far::small");
        let edit = sent(&mut app).remove(0);
        assert_eq!(edit["whole"]["model"], "far::small");
        assert_eq!(edit["whole"]["mostAgents"], 12);
    }
    #[test]
    fn prices_and_notes_are_catalogue_edits_and_success_reads_the_server_again()
     {
        use crate::welcome::server_tests::type_field;
        let mut app = with_catalogue();
        mount::<HamletTheme>(app.world_mut(), shelf());
        type_field(&mut app, "0.00", "0.25");
        let price = sent(&mut app).remove(0);
        assert_eq!(
            price["set"],
            json!({"price":{"input":0.25,"output":0.0}})
        );
        assert_eq!(price["model"], "far::small");
        incoming(
            &mut app,
            json!({"type":"reply", "to":price["id"], "kind":"notice", "text":"Saved."}),
        );
        let read = sent(&mut app).remove(0);
        assert_eq!(read["what"], "models");
        incoming(
            &mut app,
            json!({"type":"read", "to":read["id"], "models":[
                {"name":"far::small", "price":{"input":0.25,"output":0.0}, "priced":"yours", "context":null, "allowed":true, "note":"A small model."}
            ]}),
        );
        type_field(&mut app, "A small model.", "For everyday work.");
        let note = sent(&mut app).remove(0);
        assert_eq!(note["set"], json!({"note":"For everyday work."}));
        assert_eq!(note["model"], "far::small");
        app.world_mut()
            .trigger(hamlet_client::messages::Reconnected);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(sent(&mut app)[0]["what"], "models");
    }
    #[test]
    fn reconnecting_reads_again_even_if_a_catalogue_answer_was_lost()
    {
        let mut app = app();
        app.add_plugins(plugin);
        show(&mut app, false);
        let before = sent(&mut app).remove(0);
        app.world_mut()
            .trigger(hamlet_client::messages::Reconnected);
        for _ in 0..3 {
            app.update();
        }
        let after = sent(&mut app);
        assert_eq!(after.len(), 1, "a new catalogue read: {after:?}");
        assert_eq!(after[0]["what"], "models");
        assert_ne!(after[0]["id"], before["id"]);
    }
}
