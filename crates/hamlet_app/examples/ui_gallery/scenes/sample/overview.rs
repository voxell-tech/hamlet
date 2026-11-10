//! The overview and hamlet gallery scenes.

use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, ResMut};
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
            | Scene::Realm
            | Scene::Desk
            | Scene::Toasts
            | Scene::Memory
            | Scene::Character
            | Scene::Prompt
            | Scene::Merged
            | Scene::Agents
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
        "tag",
        "pin",
        "revert",
        "fork",
        "purge",
        "dream",
        "remove-scholar",
        "name-scholar",
        "restore-scholar",
        "welcome",
        "turn-away",
    ]);
    // A hamlet's page as its chief sees it, with what can be done to
    // the hamlet.
    if matches!(scene, Scene::Hamlet | Scene::Bylaws) {
        state.deeds.extend(hamlet_testing::realm::deeds(&[
            "hire",
            "worker",
            "name-hamlet",
            "move-hamlet",
            "remove-hamlet",
        ]));
    }
    state.closed = vec![protocol::ClosedHamlet {
        id: "press".into(),
        name: "The Old Press".into(),
        at: 1000,
        by: "chief".into(),
        reason: "The harvest is finished. Its lodges are kept."
            .into(),
    }];
    let mut next = 10;
    // The scholar of each hire has the hire's id.
    let mut scholars = vec![(2, "ada")];
    for (id, title, mayor, others) in [
        (
            "home",
            "Pear Tree Hollow",
            "ada",
            &["scout", "pip", "wren"][..],
        ),
        ("mill", "mill", "miller", &["sorrel", "fen"][..]),
        ("quay", "The Quay", "harbourmaster", &["tansy"][..]),
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
            state
                .agents
                .push(hamlet_testing::realm::agent(first, id, 0));
            scholars.push((first, mayor));
            next += 1;
        }
        // In the hollow scout is a lead, with pip and wren under it;
        // elsewhere every hire is under the mayor.
        let lead = next;
        for (number, name) in (1..).zip(others) {
            let mut agent =
                hamlet_testing::realm::agent(next, id, number);
            scholars.push((next, name));
            agent.parent = Some(protocol::AgentId(
                if id == "home" && number > 1 {
                    lead
                } else {
                    first
                },
            ));
            if *name == "pip" {
                agent.nickname = Some("Pippin".into());
            }
            agent.purpose = match *name {
                "scout" => "Reads the orchard's records.",
                "pip" => "Counts the pears in each row.",
                "wren" => "Mends the fences.",
                "sorrel" => "Keeps the millstones.",
                "fen" => "Weighs the flour.",
                _ => "Keeps the boats.",
            }
            .into();
            agent.busy = number == 2;
            agent.activity.doing_words =
                "Reading the orchard's notes".into();
            state.agents.push(agent);
            next += 1;
        }
    }
    busy_realm(&mut state);
    match scene {
        Scene::Agents => more_states(&mut state),
        Scene::Merged => merge_away(&mut state),
        _ => {}
    }
    state.scholars = scholars
        .into_iter()
        .map(|(id, name)| {
            hamlet_testing::realm::scholar(id, name, &[id])
        })
        .collect();
    let mut moss = hamlet_testing::realm::scholar(90, "moss", &[]);
    moss.arrival = protocol::Arrival::Waiting;
    moss.house = None;
    let mut burr = hamlet_testing::realm::scholar(91, "burr", &[]);
    burr.arrival = protocol::Arrival::Turned;
    burr.house = None;
    state.scholars.extend([moss, burr]);
    if scene != Scene::Toasts {
        // The chief holds the door, so moss waits on a petition
        // raised by scout, who made it.
        let mut wait = hamlet_testing::realm::wait("moss");
        wait.agent = Some(protocol::AgentId(10));
        wait.by = "home#1".into();
        wait.about = "scholar#90 (moss)".into();
        wait.subject = wait.about.clone();
        wait.chat = None;
        // It came in a few minutes ago.
        wait.since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
            - 5 * 60_000;
        wait.detail = protocol::WaitDetail::Newcomer {
            scholar: protocol::ScholarId(90),
        };
        wait.answers = vec![
            protocol::Choice::Welcome,
            protocol::Choice::Dismiss,
        ];
        state.waits.push(wait);
    }
    if scene == Scene::Toasts {
        // The Hand holds the door. Once the realm is on show it
        // lets moss in and a hire raises a petition.
        state.settings.newcomers = protocol::Newcomers::Hand;
        let mut next = state.clone();
        next.revision += 1;
        for scholar in &mut next.scholars {
            if scholar.name == "moss" {
                scholar.arrival = protocol::Arrival::Housed;
                scholar.house = Some(protocol::HouseAt {
                    turn: 0.0,
                    x: 40.0,
                    z: 12.0,
                });
            }
        }
        let mut wait = hamlet_testing::realm::wait("toast");
        // From an agent whose chat is not the one open, which shows
        // its own waits in full.
        if let Some(fen) =
            next.agents.iter().find(|agent| agent.name == "mill#2")
        {
            wait.agent = Some(fen.id);
            wait.by = fen.name.clone();
            if let Some(chat) = &mut wait.chat {
                chat.with = fen.id;
            }
            wait.about = "git status --short".into();
            wait.subject = wait.about.clone();
        }
        // It came in a moment ago.
        wait.since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
            - 2 * 60_000;
        next.waits.push(wait);
        world.insert_resource(Later(next, 0));
    }
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
        Scene::Hamlet | Scene::Bylaws | Scene::Agents => {
            Some(Thing::Common(first))
        }
        _ => None,
    }));
    world.insert_resource(hamlet_app::overview::Annals::received(
        first,
        "The mayor created scout to read the orchard's records. The \
         chief restored smith, whose house and notes returned."
            .into(),
    ));
    if matches!(scene, Scene::Hamlet | Scene::Bylaws | Scene::Agents)
    {
        world
            .insert_resource(hamlet_app::hub::Showing::Hamlet(first));
    }
    if scene == Scene::Merged
        && let Some(gone) = hamlet_client::realm::agent(
            world,
            protocol::AgentId(MERGED),
        )
    {
        world.insert_resource(Selected(Some(Thing::Agent(gone))));
        world.insert_resource(hamlet_app::hub::Showing::Gone(gone));
    }
    if scene == Scene::Bylaws {
        world.insert_resource(hamlet_app::overview::Page::Bylaws);
    }
}

/// A state to receive a few frames after the first, and the frames
/// counted so far.
#[derive(Resource)]
pub(in crate::scenes) struct Later(protocol::State, u32);

/// Frames to wait before the later state comes in.
const AFTER: u32 = 10;

/// Receives the later state once the first has been on show.
pub(in crate::scenes) fn later(
    mut commands: Commands,
    mut state: Option<ResMut<Later>>,
) {
    let Some(state) = state.as_mut() else {
        return;
    };
    state.1 += 1;
    if state.1 == AFTER {
        let next = state.0.clone();
        commands.queue(move |world: &mut World| {
            hamlet_client::receive(
                world,
                protocol::ServerMessage::State(next),
            );
        });
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
    let Some(lead) =
        state.agents.iter_mut().find(|agent| agent.name == "home#2")
    else {
        return;
    };
    lead.busy = true;
    lead.activity.doing = protocol::Doing::Await;
    lead.activity.doing_words = "Waiting for a worker".into();
    let lead = lead.id;
    let mut worker = hamlet_testing::realm::agent(40, "home", 9);
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
    if let Some(waiting) =
        state.agents.iter().find(|agent| agent.name == "mill#1")
    {
        let mut wait = hamlet_testing::realm::wait("ask");
        wait.agent = Some(waiting.id);
        wait.by = waiting.name.clone();
        wait.since = since;
        state.waits.push(wait);
    }
}

/// The hire merged into the hamlet's lead in the merged scene.
const MERGED: u64 = 12;

/// Makes one hire work, one stop on a fault and one wait, beside the
/// agents at rest, so every state shows in the rail and the list.
fn more_states(state: &mut protocol::State) {
    let mut set =
        |name: &str, run: &mut dyn FnMut(&mut protocol::Agent)| {
            if let Some(agent) = state
                .agents
                .iter_mut()
                .find(|agent| agent.name == name)
            {
                run(agent);
            }
        };
    set("home#3", &mut |agent| {
        agent.busy = true;
        agent.activity.doing = protocol::Doing::Run;
        agent.activity.doing_words = "Running the tests".into();
    });
    set("mill#2", &mut |agent| agent.faulted = true);
}

/// Merges the third hire of the hollow into its lead, and ends a
/// hire of the mill, so the rail lists both as gone.
fn merge_away(state: &mut protocol::State) {
    let left = |how: &str, into: Option<u64>, words: &str| {
        serde_json::from_value::<protocol::Left>(serde_json::json!({
            "at": 1_000, "by": "chief", "how": how, "into": into,
            "reason": "The work was the same.", "words": words,
        }))
        .expect("a received departure")
    };
    for agent in &mut state.agents {
        match agent.name.as_str() {
            "home#3" => {
                agent.left = Some(left("merged", Some(10), "Merged"));
            }
            "mill#1" => {
                agent.left = Some(left("ended", None, "Ended"));
            }
            _ => {}
        }
    }
}
