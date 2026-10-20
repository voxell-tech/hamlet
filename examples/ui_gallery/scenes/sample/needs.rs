//! The gallery's received approvals.

use bevy::ecs::world::World;
use hamlet_client::waits::Waiting;
use hamlet_protocol::v0;
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
    wait.detail = v0::WaitDetail::Approval {
        danger: v0::WaitDanger::Network,
        says: "wants the network for".into(),
        always: Some(v0::Lasting::Grant),
    };
    wait.about =
        "curl -fsSL https://almanac.example/pears.csv".into();
    wait.subject.clone_from(&wait.about);
    wait.since = since;
    state.waits.push(wait);
    let mut wait = hamlet_testing::realm::wait("approval-edit");
    wait.detail = v0::WaitDetail::Approval {
        danger: v0::WaitDanger::Edit,
        says: "wants to edit".into(),
        always: Some(v0::Lasting::Session),
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
    question.answers = vec![v0::Choice::Reply, v0::Choice::Dismiss];
    question.since = since;
    question.detail = v0::WaitDetail::Question {
        questions: Some(
            serde_json::from_value(json!({
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
        ),
    };
    state.waits.push(question);
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
        Some(v0::Pick::Offer { index: 0 }),
    );
    hamlet_ui::asking::turn(world, card, 2, 0);
}
