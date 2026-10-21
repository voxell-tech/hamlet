//! The received realm shared by the gallery scenes.

mod chat;
mod history;
mod inspector;
mod menu;
mod needs;
mod overview;
mod settings;
mod welcome;

use std::path::PathBuf;

use bevy::ecs::world::World;
use hamlet_client::chats::{Chat, Status, Usage};
use hamlet_client::{ViewDir, realm};
use hamlet_protocol as protocol;
use hamlet_realm::pick::{Also, Selected, Thing};
use hamlet_ui::Me;
pub(super) use welcome::first_run;

use super::Scene;

pub(super) fn setup(world: &mut World) {
    let scene = *world.resource::<Scene>();
    let mut state = hamlet_testing::realm::state();
    state.chief = "chief".into();
    state.agents[0].purpose =
        "Keeps the orchard's notes in order.".into();
    state.agents[0].spent.cost = "$0.10".into();
    hamlet_client::receive(
        world,
        protocol::ServerMessage::State(state),
    );
    let agent = realm::mayor(world).expect("the received founder");
    let to = protocol::Address {
        hamlet: "home".into(),
        agent: "mayor".into(),
    };
    let talk = chat::setup(world, scene);
    let usage = serde_json::json!({
        "calls": 3,
        "unreported": 0,
        "input": 12000,
        "cached": 0,
        "output": 400,
        "reasoning": 0,
        "ms": 500,
        "micros": 100000,
        "lastInput": 12000,
        "summary": "12.4k tokens",
    });
    let usage = serde_json::from_value::<protocol::UsageView>(usage)
        .expect("received usage");
    let session = world
        .spawn((
            Chat(protocol::ChatRef {
                kind: None,
                from: None,
                with: to.clone(),
                past: None,
            }),
            talk,
            Status::default(),
            Usage(usage),
        ))
        .id();
    world.insert_resource(Me {
        session,
        to,
        user: "chief".into(),
        agent,
        with: None,
    });
    world.init_resource::<Also>();
    world.insert_resource(Selected(Some(Thing::Resident(agent))));
    world.insert_resource(ViewDir(scratch().join("window")));
    menu::setup(world, scene);
    settings::setup(world, scene);
    inspector::setup(world, scene);
    overview::setup(world, scene);
    needs::setup(world, scene);
    if matches!(scene, Scene::History | Scene::Reading) {
        world.insert_resource(hamlet_ui::inspector::Section::History);
        history::setup(world, scene == Scene::Reading);
    }
    if scene == Scene::Chief {
        let chief = hamlet_realm::chief::arrive(
            world,
            "wren",
            hamlet_realm::chief::Look {
                hat: hamlet_realm::chief::Hat::Hood,
                ..Default::default()
            },
        );
        world.insert_resource(Selected(Some(Thing::ChiefPlace(
            chief,
        ))));
    }
}

fn scratch() -> PathBuf {
    std::env::temp_dir()
        .join(format!("hamlet-gallery-{}", std::process::id()))
}
