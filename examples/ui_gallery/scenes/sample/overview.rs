//! The overview and hamlet gallery scenes.

use bevy::ecs::world::World;
use hamlet_client::realm::Settlement;
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Selected, Thing};

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
    state.deeds = hamlet_testing::realm::deeds(&[
        "record",
        "overturn",
        "restore-hamlet",
        "erase-hamlet",
        "found",
    ]);
    state.closed = vec![protocol::ClosedHamlet {
        id: "press".into(),
        name: "The Old Press".into(),
        at: 1000,
        by: "chief".into(),
        reason: "The harvest is finished. Its houses are kept."
            .into(),
    }];
    for (id, title, mayor, others) in [
        ("home", "Pear Tree Hollow", "founder", 3),
        ("mill", "mill", "miller", 2),
        ("quay", "The Quay", "harbourmaster", 1),
    ] {
        let mut hamlet = hamlet_testing::realm::hamlet(id);
        hamlet.name = title.into();
        if id == "home" {
            hamlet.votes = vec![protocol::VoteView {
                id: 3,
                motion: "scout for mayor".into(),
                by: "chief".into(),
                open: false,
                overturnable: true,
                report: "Vote 3: scout for mayor. Two yes of three; \
                         passed."
                    .into(),
                voters: vec![],
            }];
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
    hamlet_client::receive(
        world,
        protocol::ServerMessage::State(state),
    );
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
    world.insert_resource(hamlet_ui::overview::Annals::received(
        first,
        "The mayor created scout to read the orchard's records. The \
         chief restored smith, whose house and notes returned."
            .into(),
    ));
    if scene == Scene::Hamlet {
        world.insert_resource(hamlet_ui::overview::Page::Record);
    }
    if scene == Scene::Rules {
        world.insert_resource(hamlet_ui::overview::Page::Bylaws);
    }
}
