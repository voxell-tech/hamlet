use bevy_app::App;
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use hamlet_client::realm::*;
use hamlet_client::{ClientPlugin, receive};
use hamlet_protocol::v0;

fn frames(text: &str) -> Vec<v0::ServerMessage> {
    text.lines()
        .filter_map(|line| {
            let frame =
                serde_json::from_str::<serde_json::Value>(line)
                    .unwrap();
            (frame["from"] == "server").then(|| {
                serde_json::from_value(frame["message"].clone())
                    .unwrap()
            })
        })
        .collect()
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(ClientPlugin { server: None });
    app
}

#[test]
fn recorded_hello_fills_the_realm_and_the_servers_words() {
    let mut app = app();
    let world = app.world_mut();
    let recorded = frames(include_str!(
        "../../hamlet_protocol/frames/hello.jsonl"
    ));
    let v0::ServerMessage::State(state) = &recorded[0] else {
        panic!("hello receives a whole state");
    };
    let sandbox = state.sandbox.clone();
    for message in recorded {
        receive(world, message);
    }
    let (home, settlement, shown, works, bylaws, treasury) = world.query::<(Entity, &Settlement, &Name, &Works, &Bylaws, &Treasury)>().single(world).unwrap();
    assert_eq!((&*settlement.name, shown.as_str()), ("home", "home"));
    assert_eq!(works.folder, None);
    assert_eq!(works.mode, Mode::Manual);
    assert_eq!(
        works.words,
        "Agents ask you before each edit and command."
    );
    assert_eq!(bylaws.0.most_agents, 12);
    assert_eq!(treasury.0.summary, "Hamlet today: $0.00 of $5.00");
    let (agent, citizen, name, lives, account, activity) = world.query_filtered::<(Entity, &Citizen, &Name, &LivesIn, &Account, &Activity), With<Mayor>>().single(world).unwrap();
    assert_eq!(citizen.0, 0);
    assert_eq!(name.as_str(), "founder");
    assert_eq!(lives.0, home);
    assert_eq!(account.model, "faux::faux-1");
    assert_eq!(account.summary(), "faux::faux-1, $0.00");
    assert_eq!(activity.doing, Doing::Unknown);
    assert_eq!(world.get::<Lived>(agent).unwrap().until, None);
    assert_eq!(world.resource::<Chief>().0, "chief");
    assert_eq!(world.resource::<Sandbox>().0, sandbox);
    assert_eq!(world.resource::<Settings>().0.model, "faux::faux-1");
    assert_eq!(world.resource::<Providers>().0[0].name, "faux");
    assert!(world.resource::<Grants>().0.is_empty());
    assert!(world.resource::<Deeds>().has("stop"));
    assert!(world.resource::<Deeds>().has("found"));
    assert_eq!(mayor(world), Some(agent));
    assert_eq!(named(world, "mayor"), Some(agent));
    assert_eq!(
        address_of(world, agent),
        Some(v0::Address {
            hamlet: "home".into(),
            agent: "founder".into()
        })
    );
}

#[test]
fn recorded_turn_keeps_the_agent_while_its_work_and_words_change() {
    let mut app = app();
    let world = app.world_mut();
    let mut messages = frames(include_str!(
        "../../hamlet_protocol/frames/turn.jsonl"
    ))
    .into_iter();
    receive(world, messages.next().unwrap());
    let founder = named(world, "founder").unwrap();
    let mut worked = false;
    let mut thinking = false;
    let mut thought = false;
    for message in messages {
        receive(world, message);
        assert_eq!(named(world, "founder"), Some(founder));
        if world.get::<Busy>(founder).is_some() {
            worked = true;
            let activity = world.get::<Activity>(founder).unwrap();
            if activity.doing == Doing::Think {
                assert_eq!(activity.words, "Thinking");
                thinking = true;
            }
        }
        let saying = world.get::<Saying>(founder).unwrap();
        if saying.thought && !saying.text.is_empty() {
            assert_eq!(saying.text, "I will greet the chief.");
            thought = true;
        }
    }
    assert!(worked && thought && thinking);
    assert!(world.get::<Busy>(founder).is_none());
    assert!(
        world
            .get::<Saying>(founder)
            .unwrap()
            .text
            .starts_with("Hello chief.")
    );
    assert_eq!(world.get::<Account>(founder).unwrap().cost, "<$0.01");
}

#[test]
fn recorded_approvals_join_the_founders_needs_until_closed() {
    use hamlet_client::waits::*;
    let mut app = app();
    let world = app.world_mut();
    let mut opened = 0;
    let mut previous = None;
    for message in frames(include_str!(
        "../../hamlet_protocol/frames/approval.jsonl"
    )) {
        receive(world, message);
        let founder = named(world, "founder").unwrap();
        let open = world
            .query::<(Entity, &Waiting, &RaisedBy)>()
            .iter(world)
            .map(|(entity, wait, raised)| {
                assert_eq!(raised.0, founder);
                assert!(
                    world
                        .get::<Needs>(founder)
                        .unwrap()
                        .iter()
                        .any(|need| need == entity)
                );
                assert_eq!(
                    wait.answers,
                    [Choice::Allow, Choice::Always, Choice::Deny]
                );
                if let v0::WaitDetail::Approval {
                    danger: v0::WaitDanger::Command,
                    says,
                    ..
                } = &wait.detail
                {
                    assert_eq!(says, "wants to run");
                    assert_eq!(
                        wait.line(80),
                        "mayor wants to run `printf \
                         'heading\\napproved\\n' > result.txt`"
                    );
                } else {
                    assert_eq!(
                        wait.line(80),
                        "mayor wants to edit `result.txt`"
                    );
                    assert_eq!(
                        wait.change.as_ref().unwrap().added,
                        1
                    );
                }
                assert_eq!(
                    wait.waited(wait.since + 65_000).as_secs(),
                    65
                );
                entity
            })
            .next();
        if let Some(entity) = open {
            if previous != Some(entity) {
                opened += 1;
            }
            previous = Some(entity);
        } else if let Some(entity) = previous.take() {
            assert!(world.get_entity(entity).is_err());
            assert!(
                world.get::<Needs>(founder).is_none_or(|needs| needs
                    .iter()
                    .next()
                    .is_none())
            );
        }
    }
    assert_eq!(opened, 2);
    assert!(world.query::<&Waiting>().iter(world).next().is_none());
    assert_eq!(
        Choice::quick(&[Choice::Allow, Choice::Deny], true),
        Some(Choice::Allow)
    );
}

#[test]
fn snapshots_reconcile_by_identity_across_epochs_and_changes_only_advance_the_seen_revision()
 {
    use hamlet_client::waits::{RaisedBy, Waiting};
    use hamlet_testing::realm::{agent, hamlet, state, wait};
    let mut app = app();
    let world = app.world_mut();
    let mut first = state();
    first.hamlets.push(hamlet("other"));
    first.agents.push(agent("other", 0, "scout"));
    first.waits = vec![wait("kept"), wait("gone")];
    receive(world, v0::ServerMessage::State(first.clone()));
    let founder = named(world, "founder").unwrap();
    let scout = named(world, "scout").unwrap();
    let home = world.get::<LivesIn>(founder).unwrap().0;
    let other = world.get::<LivesIn>(scout).unwrap().0;
    let wait_ids = world
        .query::<(Entity, &Waiting)>()
        .iter(world)
        .map(|(e, w)| (w.id.clone(), e))
        .collect::<std::collections::HashMap<_, _>>();
    first.epoch += 1;
    first.revision = 8;
    first.hamlets.truncate(1);
    first.agents.truncate(1);
    first.agents[0].name = "renamed".into();
    first.waits.truncate(1);
    first.waits[0].about = "a new question".into();
    receive(world, v0::ServerMessage::State(first.clone()));
    assert_eq!(named(world, "renamed"), Some(founder));
    assert_eq!(world.get::<LivesIn>(founder).unwrap().0, home);
    assert!(world.get_entity(scout).is_err());
    assert!(world.get_entity(other).is_err());
    assert!(world.get_entity(wait_ids["gone"]).is_err());
    assert_eq!(
        world.get::<Waiting>(wait_ids["kept"]).unwrap().about,
        "a new question"
    );
    assert_eq!(
        world.get::<RaisedBy>(wait_ids["kept"]).unwrap().0,
        founder
    );
    for (epoch, revision) in
        [(first.epoch, 8), (first.epoch, 7), (first.epoch - 1, 100)]
    {
        receive(
            world,
            v0::ServerMessage::Change(v0::Changes {
                epoch,
                revision,
                changes: vec![v0::Update::AgentGone {
                    ref_: first.agents[0].ref_.clone(),
                }],
            }),
        );
        assert!(world.get_entity(founder).is_ok());
    }
    assert_eq!(
        *world.resource::<Seen>(),
        Seen {
            epoch: first.epoch,
            revision: 8
        }
    );
    receive(
        world,
        v0::ServerMessage::Change(v0::Changes {
            epoch: first.epoch,
            revision: 9,
            changes: vec![v0::Update::AgentGone {
                ref_: first.agents[0].ref_.clone(),
            }],
        }),
    );
    assert!(world.get_entity(founder).is_err());
}

#[test]
fn testing_builders_give_a_hamlet_a_mayor_and_one_wait_in_three_lines()
 {
    let mut state = hamlet_testing::realm::state();
    state.waits.push(hamlet_testing::realm::wait("approval"));
    let mut app = app();
    receive(app.world_mut(), v0::ServerMessage::State(state));
    assert_eq!(
        app.world_mut()
            .query::<&hamlet_client::waits::Waiting>()
            .single(app.world())
            .unwrap()
            .line(80),
        "founder wants to run `pwd`"
    );
    let founder = mayor(app.world_mut()).unwrap();
    assert_eq!(
        app.world_mut()
            .query::<&hamlet_client::waits::RaisedBy>()
            .single(app.world())
            .unwrap()
            .0,
        founder
    );
}

#[test]
fn recorded_fault_marks_the_agent_and_keeps_the_fault_in_its_needs() {
    use hamlet_client::waits::{RaisedBy, Waiting};
    let mut app = app();
    let world = app.world_mut();
    for message in frames(include_str!(
        "../../hamlet_protocol/frames/fault.jsonl"
    )) {
        receive(world, message);
        let faulted = named(world, "founder").is_some_and(|agent| {
            world.get::<Faulted>(agent).is_some()
                && world.get::<Busy>(agent).is_none()
        });
        if faulted
            && world.query::<&Waiting>().iter(world).next().is_some()
        {
            break;
        }
    }
    let founder = named(world, "founder").unwrap();
    assert!(world.get::<Faulted>(founder).is_some());
    assert!(world.get::<Busy>(founder).is_none());
    let (wait, raised) = world
        .query::<(&Waiting, &RaisedBy)>()
        .single(world)
        .unwrap();
    assert_eq!(raised.0, founder);
    assert_eq!(wait.detail, v0::WaitDetail::Fault {});
    assert_eq!(
        wait.line(80),
        format!("{} failed: {}", wait.by, wait.about)
    );
}

#[test]
fn roster_systems_and_exclusive_callers_find_the_same_living_agents()
{
    use bevy_ecs::system::RunSystemOnce;
    use hamlet_testing::realm::{agent, hamlet, state};
    let mut app = app();
    let world = app.world_mut();
    let mut state = state();
    state.hamlets.push(hamlet("other"));
    state.agents.push(agent("other", 0, "scout"));
    receive(world, v0::ServerMessage::State(state));
    let founder = mayor(world).unwrap();
    let scout = mayor_of(world, "other").unwrap();
    assert_eq!(name_of(world, founder), "founder");
    assert_eq!(hamlet_of(world, scout), Some("other"));
    assert_eq!(
        world
            .run_system_once(|roster: Roster| (
                roster.mayor(),
                roster.named("scout"),
                roster.mayor_of("other")
            ))
            .unwrap(),
        (Some(founder), Some(scout), Some(scout))
    );
    world.entity_mut(scout).insert(Left);
    assert_eq!(named(world, "scout"), None);
    assert_eq!(mayor_of(world, "other"), None);
    assert_eq!(address_of(world, scout), None);
}

#[test]
fn streamed_words_only_change_saying_and_keep_relationship_order() {
    use hamlet_testing::realm::state;
    let mut app = app();
    let world = app.world_mut();
    let mut state = state();
    state
        .agents
        .push(hamlet_testing::realm::agent("home", 1, "scout"));
    receive(world, v0::ServerMessage::State(state.clone()));
    let founder = mayor(world).unwrap();
    let home = world.get::<LivesIn>(founder).unwrap().0;
    let before = world
        .get::<Citizens>(home)
        .unwrap()
        .iter()
        .collect::<Vec<_>>();
    world.clear_trackers();
    state.agents[0].saying.text = "Hello".into();
    receive(
        world,
        v0::ServerMessage::Change(v0::Changes {
            epoch: 1,
            revision: 1,
            changes: vec![
                v0::Update::Hamlet {
                    hamlet: state.hamlets.remove(0),
                },
                v0::Update::Agent {
                    agent: state.agents.remove(0),
                },
            ],
        }),
    );
    assert_eq!(
        world
            .query_filtered::<Entity, Changed<Saying>>()
            .iter(world)
            .collect::<Vec<_>>(),
        [founder]
    );
    assert_eq!(
        world
            .query_filtered::<Entity, Or<(
                Changed<Name>,
                Changed<Purpose>,
                Changed<Lived>,
                Changed<LivesIn>,
                Changed<Account>,
                Changed<Activity>,
                Changed<Works>,
                Changed<Bylaws>,
                Changed<Treasury>,
                Changed<Mayor>
            )>>()
            .iter(world)
            .count(),
        0
    );
    assert_eq!(
        world
            .get::<Citizens>(home)
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        before
    );
}

#[test]
fn objections_keep_the_speakers_width_in_the_one_line_subject() {
    let mut state = hamlet_testing::realm::state();
    let mut wait = hamlet_testing::realm::wait("objection");
    wait.detail = v0::WaitDetail::Objection {};
    wait.about = "heading\n12345678901234567890".into();
    wait.subject = "12345678901234567890".into();
    state.waits.push(wait);
    let mut app = app();
    receive(app.world_mut(), v0::ServerMessage::State(state));
    let world = app.world_mut();
    let wait = world
        .query::<&hamlet_client::waits::Waiting>()
        .single(world)
        .unwrap();
    assert_eq!(wait.line(10), "12345678901234567...");
}
