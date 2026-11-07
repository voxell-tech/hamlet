//! The server's catalogue and each hamlet's choices of models.

mod choice;
mod home;

use std::collections::BTreeMap;

use bevy::app::{App, PreUpdate};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::{
    resource_changed, resource_exists_and_changed,
};
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Commands, ResMut};
use bevy::ecs::world::World;
use hamlet_client::Connection;
use hamlet_client::connection::Contact;
use hamlet_client::messages::{Reconnected, Response};
use hamlet_client::outbox::Outbox;
use hamlet_protocol::client as protocol;

pub(crate) use self::choice::{efforts, of_hamlet};
pub(crate) use self::home::narrowed;
pub use self::home::{endpoints, shelf};
use crate::PanelSystems;
use crate::settings::Pane;

/// How many models a page of the catalogue shows.
const SHOWN: usize = 10;

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
        .add_systems(
            PreUpdate,
            follow
                .run_if(
                    resource_changed::<Shelf>
                        .or_eager(resource_changed::<Connection>)
                        .or_eager(
                            resource_exists_and_changed::<Pane>,
                        ),
                )
                .in_set(PanelSystems::Follow),
        );
}

/// The catalogue last received from the server.
#[derive(Resource, Default)]
pub struct Shelf {
    pub models: Vec<protocol::Listed>,
    asking: bool,
    loaded: bool,
    showing: bool,
    problem: Option<String>,
    requests: BTreeMap<protocol::RequestId, Asked>,
}

#[derive(Clone, Copy)]
enum Asked {
    Models,
    Edit,
    Default,
}

/// The catalogue's search field, and the page of it shown.
#[derive(Resource, Default)]
pub struct Draft {
    narrow: String,
    page: usize,
}

fn follow(world: &mut World) {
    let showing = world.get_resource::<Pane>() == Some(&Pane::Models);
    let shelf = world.resource::<Shelf>();
    let read =
        !shelf.asking && (!shelf.loaded || showing && !shelf.showing);
    if shelf.showing != showing {
        world.resource_mut::<Shelf>().showing = showing;
    }
    if read
        && world.resource::<Connection>().state == Contact::Connected
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
    make: impl FnOnce(protocol::RequestId) -> protocol::Edit,
) {
    let id = world
        .resource_mut::<Outbox>()
        .send(|id| protocol::ClientMessage::Edit(make(id)));
    let mut shelf = world.resource_mut::<Shelf>();
    shelf.requests.insert(id, Asked::Edit);
    shelf.problem = None;
}

fn problem() -> impl hamlet_ui::View {
    hamlet_ui::styles::note(
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
                read_models(world, false);
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
    use bevy::ecs::entity::Entity;
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::text::EditableText;
    use bevy_fynix::mount;
    use hamlet_ui::theme::HamletTheme;
    use serde_json::json;

    use super::*;
    use crate::testing::{
        all_words, connect, incoming, press_and_update, sent,
        welcome_app,
    };

    #[test]
    fn the_models_page_reads_the_catalogue_and_shows_the_servers_prices()
     {
        let mut app = welcome_app();
        app.add_plugins(plugin);
        connect(&mut app, false);
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
                {"name":"far::large", "price":{"input":3.0,"output":15.0}, "priced":"listed", "context":200000, "defaultEffort":null, "efforts":[], "fast":false, "note":"For hard questions."}
            ]}),
        );
        mount::<HamletTheme>(app.world_mut(), shelf());
        let shown = all_words(&mut app);
        for text in ["far::large", "3.00", "15.00", "listed", "200k"]
        {
            assert!(shown.contains(text), "{shown}");
        }
    }
    fn with_catalogue() -> App {
        let mut app = welcome_app();
        app.add_plugins((plugin, crate::roll::plugin));
        connect(&mut app, false);
        let read = sent(&mut app).remove(0);
        incoming(
            &mut app,
            json!({"type":"read", "to":read["id"], "models":[
                {"name":"far::small", "price":null, "priced":"unknown", "context":null, "defaultEffort":null, "efforts":[], "fast":false, "note":""}
            ]}),
        );
        app
    }
    /// Types `text` into the field that holds `was`, as the chief
    /// does: focus, edit, then leave it.
    fn type_field(app: &mut App, was: &str, text: &str) {
        let input = app
            .world_mut()
            .query::<(Entity, &EditableText)>()
            .iter(app.world())
            .find(|(_, value)| value.value() == was)
            .map(|(entity, _)| entity)
            .expect("editable field");
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(input, FocusCause::Pressed);
        app.update();
        app.world_mut()
            .get_mut::<EditableText>(input)
            .unwrap()
            .editor_mut()
            .set_text(text);
        app.world_mut().resource_mut::<InputFocus>().clear();
        app.update();
    }

    #[test]
    fn allowance_refresh_and_a_refused_catalogue_edit_go_through_the_server()
     {
        let mut app = with_catalogue();
        mount::<HamletTheme>(app.world_mut(), shelf());
        type_field(&mut app, "0.00", "0.25");
        let edit = sent(&mut app).remove(0);
        assert_eq!(edit["doc"], "catalogue");
        assert_eq!(edit["model"], "far::small");
        incoming(
            &mut app,
            json!({"type":"reply", "to":edit["id"], "kind":"error", "text":"The catalogue could not be saved."}),
        );
        assert!(
            all_words(&mut app)
                .contains("The catalogue could not be saved.")
        );
        press_and_update(&mut app, "List models");
        let read = sent(&mut app).remove(0);
        assert_eq!(read["what"], "models");
        assert_eq!(read["refresh"], true);
    }

    #[test]
    fn a_hamlets_list_and_default_keep_the_rest_of_its_received_limits()
     {
        let mut app = with_catalogue();
        let mayor =
            hamlet_client::realm::first_mayor(app.world()).unwrap();
        let hamlet = app
            .world()
            .get::<hamlet_client::realm::LivesIn>(mayor)
            .unwrap()
            .0;
        mount::<HamletTheme>(app.world_mut(), of_hamlet(hamlet));
        press_and_update(&mut app, "faux::faux-1");
        press_and_update(&mut app, "far::small");
        let edit = sent(&mut app).remove(0);
        assert_eq!(edit["whole"]["model"], "far::small");
        assert_eq!(edit["whole"]["road"]["most"], 24);
        assert_eq!(edit["whole"]["budget"]["cost"], 5.0);
        incoming(
            &mut app,
            json!({"type":"reply", "to":edit["id"], "kind":"error", "text":"This model was refused."}),
        );
        assert!(
            all_words(&mut app).contains("This model was refused.")
        );
    }
    #[test]
    fn prices_are_catalogue_edits_and_success_reads_the_server_again()
    {
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
        app.world_mut().trigger(Reconnected);
        app.update();
        assert_eq!(sent(&mut app)[0]["what"], "models");
    }
    #[test]
    fn reconnecting_reads_again_even_if_a_catalogue_answer_was_lost()
    {
        let mut app = welcome_app();
        app.add_plugins(plugin);
        connect(&mut app, false);
        let before = sent(&mut app).remove(0);
        app.world_mut().trigger(Reconnected);
        app.update();
        let after = sent(&mut app);
        assert_eq!(after.len(), 1, "a new catalogue read: {after:?}");
        assert_eq!(after[0]["what"], "models");
        assert_ne!(after[0]["id"], before["id"]);
    }
}
