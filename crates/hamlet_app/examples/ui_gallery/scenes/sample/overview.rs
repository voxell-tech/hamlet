//! The overview and hamlet gallery scenes.

use bevy::ecs::world::World;
use hamlet_client::realm::HamletId;
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Selected, Thing};

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if !matches!(
        scene,
        Scene::Overview
            | Scene::Hamlet
            | Scene::Bylaws
            | Scene::Hub
            | Scene::Scholar
            | Scene::Home
            | Scene::Desk
            | Scene::Toasts
    ) {
        return;
    }
    let mut state = hamlet_testing::realm::state();
    state.chief = "chief".into();
    state.deeds = hamlet_testing::realm::deeds(&[
        "record",
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
    let mut next = 10;
    for (id, title, mayor, others) in [
        ("home", "Pear Tree Hollow", "ada", 3),
        ("mill", "mill", "miller", 2),
        ("quay", "The Quay", "harbourmaster", 1),
    ] {
        let first = if id == "home" { 2 } else { next };
        let mut hamlet = hamlet_testing::realm::hamlet(id, first);
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
            state.agents.push(hamlet_testing::realm::agent(
                first, id, 0, mayor,
            ));
            next += 1;
        }
        for number in 1..=others {
            let mut agent = hamlet_testing::realm::agent(
                next,
                id,
                number,
                &format!("{id}-hand-{number}"),
            );
            agent.purpose = "Keeps a row of the orchard.".into();
            agent.busy = number == 2;
            agent.activity.doing_words =
                "Reading the orchard's notes".into();
            state.agents.push(agent);
            next += 1;
        }
    }
    busy_realm(&mut state);
    state.scholars = state
        .agents
        .iter()
        .filter_map(|agent| agent.scholar.map(|id| (id, agent)))
        .map(|(id, agent)| {
            let name = agent.name.split('@').next().unwrap_or("");
            hamlet_testing::realm::scholar(id.0, name, &[agent.id.0])
        })
        .collect();
    let mut moss = hamlet_testing::realm::scholar(90, "moss", &[]);
    moss.arrival = protocol::Arrival::Waiting;
    moss.house = None;
    let mut burr = hamlet_testing::realm::scholar(91, "burr", &[]);
    burr.arrival = protocol::Arrival::Turned;
    burr.house = None;
    state.scholars.extend([moss, burr]);
    if let Some(ada) =
        state.scholars.iter_mut().find(|s| s.name == "ada")
    {
        ada.versions = vec![
            protocol::VersionView {
                at: 1000,
                name: "v1".into(),
                note: "Learned the rows".into(),
            },
            protocol::VersionView {
                at: 2000,
                name: "v2".into(),
                note: "Learned the press".into(),
            },
        ];
        ada.skills = vec!["orchard".into()];
    }
    hamlet_client::receive(
        world,
        protocol::ServerMessage::State(state),
    );
    let first = world
        .query::<(bevy::ecs::entity::Entity, &HamletId)>()
        .iter(world)
        .find(|(_, hamlet)| hamlet.0 == "home")
        .map(|(entity, _)| entity)
        .unwrap();
    world.insert_resource(Selected(match scene {
        Scene::Hamlet | Scene::Bylaws => Some(Thing::Common(first)),
        _ => None,
    }));
    world.insert_resource(hamlet_app::overview::Annals::received(
        first,
        "The mayor created scout to read the orchard's records. The \
         chief restored smith, whose house and notes returned."
            .into(),
    ));
    if scene == Scene::Hamlet {
        world.insert_resource(hamlet_app::overview::Page::Record);
    }
    if scene == Scene::Bylaws {
        world.insert_resource(hamlet_app::overview::Page::Bylaws);
    }
}

/// Makes the realm one at work: a hire that waits on a worker,
/// another that waits for the chief, and the Hand at its desk.
fn busy_realm(state: &mut protocol::State) {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if let Some(hand) =
        state.agents.iter_mut().find(|agent| agent.name == "hand")
    {
        hand.activity.doing_words = "Founding a hamlet".into();
        hand.busy = true;
    }
    let Some(lead) = state
        .agents
        .iter_mut()
        .find(|agent| agent.name == "home-hand-2@home")
    else {
        return;
    };
    lead.busy = true;
    lead.activity.doing = protocol::Doing::Await;
    lead.activity.doing_words = "Waiting for a worker".into();
    let lead = lead.id;
    let mut worker = hamlet_testing::realm::agent(40, "home", 9, "x");
    worker.kind = protocol::AgentKind::Worker;
    worker.name = "worker#40".into();
    worker.number = None;
    worker.scholar = None;
    worker.parent = Some(lead);
    worker.busy = true;
    worker.purpose = "Count the pears in row four.".into();
    worker.activity.doing_words =
        "Reading the orchard's notes".into();
    state.agents.push(worker);
    if let Some(waiting) = state
        .agents
        .iter()
        .find(|agent| agent.name == "mill-hand-1@mill")
    {
        let mut wait = hamlet_testing::realm::wait("ask");
        wait.agent = Some(waiting.id);
        wait.by = waiting.name.clone();
        wait.since = since;
        state.waits.push(wait);
    }
}
