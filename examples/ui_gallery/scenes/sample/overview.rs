//! The overview and hamlet gallery scenes.

use bevy::ecs::world::World;
use hamlet_client::realm::Settlement;
use hamlet_protocol::v0;
use hamlet_world::pick::{Selected, Thing};

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if !matches!(
        scene,
        Scene::Overview | Scene::Hamlet | Scene::Rules
    ) {
        return;
    }
    let mut state = hamlet_testing::realm::state();
    state.chief = "chief".into();
    for (id, title, mayor, others) in [
        ("home", "Pear Tree Hollow", "founder", 3),
        ("mill", "mill", "miller", 2),
        ("quay", "The Quay", "harbourmaster", 1),
    ] {
        let mut hamlet = hamlet_testing::realm::hamlet(id);
        hamlet.name = title.into();
        if id == "home" {
            hamlet.folder = Some("/projects/orchard-almanac".into());
            hamlet.mode_words = "Agents ask before each edit and \
                                 command on the server."
                .into();
            hamlet.budget.lines = vec![
                "Today: $1.25 of $5.00".into(),
                "Model time: 4 minutes".into(),
            ];
            hamlet.before = vec![
                "/projects/pear-press".into(),
                "/projects/cider-ledger".into(),
            ];
        }
        if id == "home" {
            state.hamlets[0] = hamlet;
        } else {
            state.hamlets.push(hamlet);
        }
        if id != "home" {
            state
                .agents
                .push(hamlet_testing::realm::agent(id, 0, mayor));
        }
        for number in 1..=others {
            let mut agent = hamlet_testing::realm::agent(
                id,
                number,
                &format!("{id}-hand-{number}"),
            );
            agent.purpose = "Keeps a row of the orchard.".into();
            agent.busy = number == 2;
            agent.activity.doing_words =
                "Reading the orchard's notes".into();
            state.agents.push(agent);
        }
    }
    hamlet_client::receive(world, v0::ServerMessage::State(state));
    let first = world
        .query::<(bevy::ecs::entity::Entity, &Settlement)>()
        .iter(world)
        .find(|(_, hamlet)| hamlet.name == "home")
        .map(|(entity, _)| entity)
        .unwrap();
    world.insert_resource(Selected(match scene {
        Scene::Hamlet | Scene::Rules => Some(Thing::Common(first)),
        _ => None,
    }));
    if scene == Scene::Rules {
        world.insert_resource(hamlet_ui::overview::Page::Bylaws);
    }
}
