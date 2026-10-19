//! The gallery's received approvals.

use bevy::ecs::world::World;
use hamlet_client::waits::Waiting;
use hamlet_protocol::{asking, v0};
use serde_json::json;

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if scene != Scene::Waiting {
        return;
    }
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let mut state = hamlet_testing::realm::state();
    state.chief = "chief".into();
    let mut wait = hamlet_testing::realm::wait("approval-network");
    if let v0::Wait::Approval {
        danger,
        about,
        since: at,
        ..
    } = &mut wait
    {
        *danger = v0::WaitDanger::Network;
        *about =
            "curl -fsSL https://almanac.example/pears.csv".into();
        *at = since;
    }
    state.waits.push(wait);
    let mut wait = hamlet_testing::realm::wait("approval-edit");
    if let v0::Wait::Approval {
        danger,
        says,
        about,
        change,
        since: at,
        ..
    } = &mut wait
    {
        *danger = v0::WaitDanger::Edit;
        *says = "wants to edit".into();
        *about = "src/almanac.rs\nReplace the pear count.".into();
        *change = Some(super::chat::change());
        *at = since;
    }
    state.waits.push(wait);
    let asked = asking::read(
        &json!({
            "message": "Before I plan the orchard's winter.",
            "questions": [
                {"question": "Which trees come first?",
                 "description": "The frost reaches the low rows a week early.",
                 "options": [
                    {"title": "Pears", "description": "Bruise in the cold"},
                    {"title": "Apples"},
                    {"title": "Both", "description": "Takes two more days"}
                 ]},
                {"question": "How much may the straw cost?",
                 "description": "Per bale."}
            ]
        })
        .to_string(),
    )
    .expect("gallery questions");
    let mut question = serde_json::to_value(
        hamlet_testing::realm::wait("questions"),
    )
    .expect("gallery wait");
    question["kind"] = json!("question");
    question["danger"] = json!(null);
    question["says"] = json!("asks");
    question["about"] = json!(asked.about());
    question["answers"] = json!(["reply", "dismiss"]);
    question["since"] = json!(since);
    question["questions"] = serde_json::to_value(asked)
        .expect("gallery question payload");
    state.waits.push(
        serde_json::from_value(question).expect("question wait"),
    );
    hamlet_client::receive(world, v0::ServerMessage::State(state));
    let card = world
        .query::<(bevy::ecs::entity::Entity, &Waiting)>()
        .iter(world)
        .find(|(_, wait)| wait.id == "questions")
        .map(|(entity, _)| entity)
        .expect("received question card");
    hamlet_ui::asking::choose(
        world,
        card,
        2,
        0,
        Some(asking::Pick::Offer(0)),
    );
    hamlet_ui::asking::turn(world, card, 2, 0);
}
