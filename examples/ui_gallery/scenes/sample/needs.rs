//! The gallery's received approvals.

use bevy::ecs::world::World;
use hamlet_client::waits::Waiting;
use hamlet_protocol::client as protocol;
use serde_json::json;

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if scene != Scene::Waiting {
        return;
    }
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut state = hamlet_testing::realm::state();
    state.chief = "chief".into();
    let mut wait = hamlet_testing::realm::wait("approval-network");
    wait.detail = protocol::WaitDetail::Approval {
        danger: protocol::WaitDanger::Network,
        says: "wants the network for".into(),
        always: Some(protocol::Lasting::Grant),
    };
    wait.about =
        "curl -fsSL https://almanac.example/pears.csv".into();
    wait.subject.clone_from(&wait.about);
    wait.since = since;
    state.waits.push(wait);
    let mut wait = hamlet_testing::realm::wait("approval-edit");
    wait.detail = protocol::WaitDetail::Approval {
        danger: protocol::WaitDanger::Edit,
        says: "wants to edit".into(),
        always: Some(protocol::Lasting::Session),
    };
    wait.about = "src/almanac.rs\nReplace the pear count.".into();
    wait.subject = "src/almanac.rs".into();
    wait.change = Some(super::chat::change());
    wait.since = since;
    state.waits.push(wait);
    let mut question = hamlet_testing::realm::wait("questions");
    question.about = "Before I plan the orchard's winter.\n1. Which \
                      trees come first?\n2. How much may the straw \
                      cost?"
        .into();
    question.subject = "Before I plan the orchard's winter.".into();
    question.answers =
        vec![protocol::Choice::Reply, protocol::Choice::Dismiss];
    question.since = since;
    question.detail = protocol::WaitDetail::Question {
        questions: serde_json::from_value(json!({
                "message": "Before I plan the orchard's winter.",
                "questions": [
                    {"question": "Which trees come first?",
                     "description": "The frost reaches the low rows a week early.",
                     "options": [
                        {"title": "Pears", "description": "Bruise in the cold"},
                        {"title": "Apples", "description": ""},
                        {"title": "Both", "description": "Takes two more days"}
                     ],
                     "text_input": false},
                    {"question": "How much may the straw cost?",
                     "description": "Per bale.",
                     "options": [],
                     "text_input": true}
                ]
            }))
            .expect("gallery questions"),
    };
    state.waits.push(question);
    hamlet_client::receive(
        world,
        protocol::ServerMessage::State(state),
    );
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
        Some(protocol::Pick::Offer { index: 0 }),
    );
    hamlet_ui::asking::turn(world, card, 2, 0);
}
