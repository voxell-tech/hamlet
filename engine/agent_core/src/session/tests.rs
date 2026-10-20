use std::time::Duration;

use bevy_ecs::name::Name;
use hamlet_rules::hamlet::{FOUNDER, MAYOR};
use hamlet_rules::lifecycle::Who;
use hamlet_sandbox::Unsandboxed;
use serde_json::{Value, json};
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

use super::output::note_work;
use super::*;
use crate::agent::Ruling;
use crate::agents::{Activity, Busy, Citizen, Left, Mayor};
use crate::core::Settings;
use crate::testing::{
    self, Place, bash, call, limit, place, says, with_agent,
    with_mayor,
};
use crate::tools::{Leave, Reach};

/// An app with sessions in a fresh home.
fn app(tag: &str, rt: &Runtime, replies: Vec<Value>) -> (App, Place) {
    let home = place(&format!("session-{tag}"));
    let app = testing::app(&home, rt, &Unsandboxed, replies);
    (app, home)
}

/// Types `text` to the first mayor.
fn say(app: &mut App, text: &str) {
    testing::say(app, &with_mayor(), text);
}

/// Ticks until the first mayor has answered.
fn settle(app: &mut App) -> Entity {
    testing::settle(app, &with_mayor())
}

/// Types `text` to the session `key` and waits for the answer.
fn send(app: &mut App, key: &SessionKey, text: &str) -> Entity {
    testing::say(app, key, text);
    testing::settle(app, key)
}

/// A hamlet with a librarian.
fn with_librarian(home: &Place) -> Hamlet {
    let hamlet = Hamlet::new(home.root.clone());
    hamlet
        .create("librarian", "Keeps the reading list.", "test")
        .unwrap();
    hamlet
}

#[test]
fn a_turn_runs_through_the_worker_into_the_transcript() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) =
        app("turn", &rt, vec![bash("echo hi > f.txt"), says("done")]);
    say(&mut app, "make a file");
    let session = settle(&mut app);
    let t = app.world().get::<Transcript>(session).unwrap();
    assert!(matches!(t.get(0), Some(Entry::User { .. })));
    assert!(matches!(
        t.get(1),
        Some(Entry::Tool { exit: Some(0), .. })
    ));
    assert_eq!(t.get(2), Some(&Entry::Assistant("done".into())));
    assert!(home.workspace.join("f.txt").exists());
    // Two model calls ran; the replay provider reports no token
    // counts.
    let usage = app.world().get::<Usage>(session).unwrap();
    assert_eq!((usage.calls, usage.unreported), (2, 2));
    shutdown(app.world_mut());
}

#[test]
fn the_network_switch_is_read_at_every_tool_call() {
    let rt = Runtime::new().unwrap();
    let (mut app, _home) = app("net", &rt, vec![]);
    let ruling = |app: &mut App| {
        let (answer, mut ruled) = oneshot::channel();
        rule(
            app.world_mut(),
            ToolStarting {
                session: Entity::PLACEHOLDER,
                who: MAYOR.into(),
                name: "bash".into(),
                args: "{}".into(),
                reach: Reach::Other,
                deny: None,
                leave: Leave::default(),
                answer: Some(answer),
            },
        );
        ruled.try_recv().unwrap()
    };
    assert_eq!(ruling(&mut app), Ruling::Go(Leave::default()));
    app.world_mut().resource_mut::<Settings>().config.allow_net =
        true;
    assert_eq!(
        ruling(&mut app),
        Ruling::Go(Leave {
            net: true,
            reserved: false
        })
    );
}

#[test]
fn a_stop_ends_a_turn_not_yet_begun_and_spares_the_next() {
    // Nothing runs on this runtime until it is driven, so the
    // stop comes before the worker has taken the first job.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (mut app, _home) =
        app("stop-unbegun", &rt, vec![says("second")]);
    say(&mut app, "first");
    let session = find(app.world_mut(), &with_mayor()).unwrap();
    app.world_mut().trigger(Stop {
        session,
        reason: "enough".into(),
    });
    say(&mut app, "second");
    for _ in 0..500 {
        rt.block_on(async {
            tokio::time::sleep(Duration::from_millis(5)).await
        });
        app.update();
        if !app.world().get::<Status>(session).unwrap().working() {
            break;
        }
    }
    let t = app.world().get::<Transcript>(session).unwrap();
    assert!(t.items().iter().any(|item| {
        item.entry
            == Entry::Notice(
                "Stopped: enough (1 queued dropped)".into(),
            )
    }));
    assert_eq!(t.last(), Some(&Entry::Assistant("second".into())));
}

#[test]
fn a_changed_setting_reaches_the_next_job() {
    let rt = Runtime::new().unwrap();
    let (mut app, _home) = app(
        "brief",
        &rt,
        vec![says("one"), says("two"), says("nothing to save")],
    );
    let reflected = |app: &App, session: Entity| {
        app.world()
            .get::<Transcript>(session)
            .unwrap()
            .items()
            .iter()
            .any(|i| i.entry == Entry::Notice("reflecting".into()))
    };
    say(&mut app, "hi");
    let session = settle(&mut app);
    assert!(!reflected(&app, session));
    app.world_mut()
        .resource_mut::<Settings>()
        .config
        .reflect_every = 1;
    say(&mut app, "again");
    settle(&mut app);
    assert!(reflected(&app, session));
    shutdown(app.world_mut());
}

#[test]
fn a_conversation_follows_its_agent_and_shows_its_work() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app("work", &rt, vec![]);
    let hamlet = with_librarian(&home);
    app.update();
    let world = app.world_mut();
    let librarian = agents::named(world, "librarian").unwrap();
    let mayor = agents::mayor(world).unwrap();
    let with_mayor = ensure(world, &with_mayor());
    let with_librarian = ensure(world, &with_agent("librarian"));
    let talks = |world: &World, session| {
        world.get::<TalksTo>(session).map(|t| t.0)
    };
    assert_eq!(talks(world, with_mayor), Some(mayor));
    assert_eq!(talks(world, with_librarian), Some(librarian));

    let tool = |name: &str| AgentEvent::Tool {
        name: name.into(),
        summary: "{}".into(),
        change: None,
    };
    let state = |world: &World, agent| {
        (
            world.get::<Busy>(agent).is_some(),
            world.get::<Activity>(agent).unwrap().step,
        )
    };
    // Its own turn is its own work.
    let event = Reply::Event(tool("bash"));
    note_work(world, with_librarian, false, &event);
    assert_eq!(state(world, librarian), (true, 1));
    let done = Reply::Done(String::new());
    note_work(world, with_librarian, false, &done);
    assert_eq!(state(world, librarian), (false, 0));

    // Asked by the mayor, it is busy while the mayor waits.
    let event = Reply::Event(tool("ask_agent"));
    note_work(world, with_mayor, false, &event);
    assert_eq!(state(world, mayor), (true, 1));
    let asked = Reply::Event(AgentEvent::Agent {
        name: "librarian".into(),
        event: Box::new(tool("bash")),
    });
    note_work(world, with_mayor, false, &asked);
    assert_eq!(state(world, librarian), (true, 1));
    note_work(world, with_mayor, false, &done);
    assert_eq!(state(world, librarian), (false, 0));
    assert_eq!(state(world, mayor), (false, 0));

    // A conversation with an agent that left is with nobody,
    // and with the same agent again when it is back.
    note_work(world, with_librarian, false, &event);
    hamlet
        .remove("librarian", "x", Who::Chief("alice"))
        .unwrap();
    app.update();
    let world = app.world_mut();
    assert_eq!(talks(world, with_librarian), None);
    assert_eq!(state(world, librarian), (false, 0));
    hamlet
        .restore("librarian", None, "x", Who::Chief("alice"))
        .unwrap();
    app.update();
    let world = app.world();
    assert_eq!(talks(world, with_librarian), Some(librarian));
}

#[test]
fn a_chief_asks_the_hamlet_to_vote_on_its_mayor() {
    let rt = Runtime::new().unwrap();
    let ballot = |id: &str| {
        json!({"role": "assistant", "content": "", "tool_calls": [
            {"id": id, "type": "function", "function": {
                "name": "cast_ballot",
                "arguments": json!({"choice": "yes",
                    "reason": "From my own work.",
                    "evidence": ""}).to_string()}}]})
    };
    let (mut app, home) = app(
        "chief-vote",
        &rt,
        vec![
            // The mayor's answer, then a ballot from each agent.
            says("I did my best."),
            ballot("b1"),
            ballot("b2"),
            ballot("b3"),
        ],
    );
    let hamlet = Hamlet::new(home.root.clone());
    for name in ["librarian", "scout", "smith"] {
        hamlet.create(name, "Works.", "test").unwrap();
    }
    // It says how, when nobody is named.
    say(&mut app, "/vote");
    let session = settle(&mut app);
    let last = |app: &App| {
        let said = app.world().get::<Transcript>(session).unwrap();
        said.items().last().map(|item| item.entry.clone())
    };
    assert!(matches!(
        last(&app),
        Some(Entry::Notice(how)) if how.contains("/vote <agent>")
    ));
    say(&mut app, "/vote scout It keeps the list best.");
    settle(&mut app);
    assert_eq!(hamlet.mayor().name, "scout");
    let vote = hamlet_rules::vote::find(&hamlet, 1).unwrap();
    assert_eq!(vote.by, "chief:alice");
    assert_eq!(vote.answer.as_deref(), Some("I did my best."));
    shutdown(app.world_mut());
}

#[test]
fn new_archives_the_log_and_clears_the_transcript() {
    let rt = Runtime::new().unwrap();
    // Second response answers the reflection that /new forces.
    let (mut app, home) =
        app("new", &rt, vec![says("hello"), says("nothing to save")]);
    say(&mut app, "hi");
    settle(&mut app);
    say(&mut app, "/new");
    let session = settle(&mut app);
    let t = app.world().get::<Transcript>(session).unwrap();
    assert_eq!(t.items().len(), 1);
    assert!(matches!(t.items()[0].entry, Entry::Notice(_)));
    let logs: Vec<String> = std::fs::read_dir(home.sessions())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(logs.len(), 1, "{logs:?}");
    assert!(
        logs[0].starts_with("test_alice_main.")
            && logs[0].ends_with(".jsonl")
            && logs[0] != "test_alice_main.jsonl"
    );
    assert!(app.world().get::<Jobs>(session).is_none());
    // It began now, and is read as having begun when the last
    // one was put away at the next launch.
    let began = app.world().get::<Began>(session).unwrap().0;
    let log = home.sessions().join("test_alice_main.jsonl");
    let read = Began::of(&log, &Transcript::default()).0;
    assert!(read > 0 && read <= began, "{read} {began}");
    shutdown(app.world_mut());
}

#[test]
fn a_line_sent_while_a_session_is_put_away_does_not_hang_it() {
    let rt = Runtime::new().unwrap();
    let (mut app, _home) = app(
        "new-then-more",
        &rt,
        vec![
            says("hello"),
            says("nothing to save"),
            says("hello again"),
        ],
    );
    say(&mut app, "hi");
    settle(&mut app);
    say(&mut app, "/new");
    say(&mut app, "more");
    let session = settle(&mut app);
    // The next line finds a worker that reads it.
    say(&mut app, "again");
    settle(&mut app);
    let t = app.world().get::<Transcript>(session).unwrap();
    assert!(
        t.items().iter().any(|item| matches!(
            &item.entry,
            Entry::Assistant(text) if text.contains("hello again")
        )),
        "the line after was not answered"
    );
    shutdown(app.world_mut());
}

#[test]
fn resume_goes_back_to_a_past_session_and_keeps_the_one_left() {
    let rt = Runtime::new().unwrap();
    let nothing = says("nothing to save");
    let (mut app, home) = app(
        "resume",
        &rt,
        vec![says("hello"), nothing.clone(), says("again"), nothing],
    );
    let logs = |home: &Home| {
        let mut logs = std::fs::read_dir(home.sessions())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        logs.sort();
        logs
    };
    say(&mut app, "hi");
    settle(&mut app);
    say(&mut app, "/new");
    settle(&mut app);
    let first = logs(&home).remove(0);
    let number = first.split('.').nth(1).unwrap().to_string();
    say(&mut app, "more");
    settle(&mut app);

    say(&mut app, &format!("/resume {number}"));
    let session = settle(&mut app);
    let t = app.world().get::<Transcript>(session).unwrap();
    let said = t
        .items()
        .iter()
        .filter_map(|item| match &item.entry {
            Entry::User { text, .. } => Some(text.as_str()),
            Entry::Assistant(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    // The reflection that closed it is part of its log.
    assert_eq!(said, ["hi", "hello", "nothing to save"]);
    assert!(app.world().get::<Jobs>(session).is_none());

    // The session left behind is archived.
    let now = logs(&home);
    assert_eq!(now.len(), 2, "{now:?}");
    assert!(!now.contains(&first));
    let left = now
        .iter()
        .find(|name| *name != "test_alice_main.jsonl")
        .unwrap();
    let left =
        std::fs::read_to_string(home.sessions().join(left)).unwrap();
    assert!(left.contains("more") && !left.contains("hello"));
    shutdown(app.world_mut());
}

#[test]
fn a_chiefs_instruction_runs_in_the_agents_house_and_is_owed_until_answered()
 {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "direct",
        &rt,
        vec![bash("echo Dune >> list.txt"), says("Added.")],
    );
    let hamlet = with_librarian(&home);
    limit(&home, "[leaving]\nprotected_minutes = 10\n");
    let key = with_agent("librarian");
    testing::say(&mut app, &key, "Add Dune.");
    // From the moment it is given the mayor may not remove it,
    // whether the turn is running or just over.
    let refused = hamlet
        .remove("librarian", "Not needed.", Who::Mayor)
        .unwrap_err();
    assert!(refused.to_string().contains("to avoid a chief's task"));
    // The mayor is told.
    assert!(
        hamlet
            .prompt("")
            .contains("alice told librarian: Add Dune.")
    );

    testing::settle(&mut app, &key);
    let house = home.root.join("agents/librarian");
    assert!(house.join("workspace/list.txt").exists());
    assert!(!home.workspace.join("list.txt").exists());
    let log = "sessions/test_alice_agent_librarian.jsonl";
    assert!(house.join(log).exists());
    let refused = hamlet
        .remove("librarian", "Not needed.", Who::Mayor)
        .unwrap_err();
    assert!(refused.to_string().contains("only just ended"));
    limit(&home, "[leaving]\nprotected_minutes = 0\n");
    hamlet
        .remove("librarian", "Not needed.", Who::Mayor)
        .unwrap();
    shutdown(app.world_mut());
}

#[test]
fn quitting_does_not_wait_for_a_running_turn() {
    let rt = Runtime::new().unwrap();
    let (mut app, _home) = app("quit", &rt, vec![bash("sleep 30")]);
    say(&mut app, "wait a long time");
    // Let the turn reach its command.
    for _ in 0..30 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    let started = Instant::now();
    app.world_mut().write_message(AppExit::Success);
    app.update();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(app.world().resource::<Workers>().0.is_empty());
    rt.shutdown_background();
}
#[test]
fn a_told_agent_works_in_its_own_session_and_its_reply_comes_back() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "tell",
        &rt,
        vec![
            call(
                "tell_agent",
                // A letter is words, whatever it opens with.
                json!({"name": "librarian", "message": "/new"}),
            ),
            // The mayor's own turn ends and the librarian
            // answers, in either order.
            says("ok"),
            says("ok"),
            // The mayor's turn on the reply.
            says("Noted."),
        ],
    );
    with_librarian(&home);
    say(&mut app, "have the librarian add Dune");

    let replied = |app: &mut App| {
        let world = app.world_mut();
        let mayor =
            find(world, &with_mayor()).expect("session exists");
        let transcript = world.get::<Transcript>(mayor).unwrap();
        let heard = transcript.items().iter().any(|i| {
            matches!(
                &i.entry,
                Entry::User { user, text }
                    if user == "librarian"
                        && text.starts_with("Reply from `librarian`: ok")
            )
        });
        heard && !world.get::<Status>(mayor).unwrap().working()
    };
    testing::until(&mut app, "the reply to come back", replied);

    // The librarian worked in a session of its own, in its house.
    assert!(
        home.root
            .join(
                "agents/librarian/sessions/\
                 hamlet_mayor_agent_librarian.jsonl"
            )
            .exists()
    );
    let mayor = find(app.world_mut(), &with_mayor()).unwrap();
    assert_eq!(
        app.world().get::<Transcript>(mayor).unwrap().last(),
        Some(&Entry::Assistant("Noted.".into()))
    );
    shutdown(app.world_mut());
}

#[test]
fn an_asked_agent_keeps_a_conversation_with_who_asked() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "ask",
        &rt,
        vec![
            call(
                "ask_agent",
                json!({"name":"librarian","message":"Is Dune in?"}),
            ),
            says("It is."),
            says("Yes."),
        ],
    );
    with_librarian(&home);
    say(&mut app, "is Dune on the list?");
    settle(&mut app);

    let world = app.world_mut();
    let key = SessionKey::asked("mayor", "librarian", HOME);
    let said = |world: &mut World| {
        let asked = find(world, &key).expect("it was asked");
        world
            .get::<Transcript>(asked)
            .unwrap()
            .items()
            .iter()
            .map(|item| item.entry.clone())
            .collect::<Vec<_>>()
    };
    let talk = [
        Entry::User {
            user: "mayor".into(),
            text: "Is Dune in?".into(),
        },
        Entry::Assistant("It is.".into()),
    ];
    assert_eq!(said(world), talk);
    let asked = find(world, &key).unwrap();
    let librarian = agents::named(world, "librarian");
    assert_eq!(world.get::<TalksTo>(asked).map(|t| t.0), librarian);

    // Read again from the librarian's own log, it is the same.
    world.despawn(asked);
    let mayor = agents::mayor(world).unwrap();
    gather(world, mayor);
    assert_eq!(said(world), talk);
    shutdown(app.world_mut());
}

/// The names of the living agents besides the mayor.
fn residents(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    world
        .query_filtered::<&Name, (
            With<Citizen>,
            Without<Mayor>,
            Without<Left>,
        )>()
        .iter(world)
        .map(|n| n.as_str().to_string())
        .collect()
}

/// The names of the agents that left.
fn departed(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    world
        .query_filtered::<&Name, With<Left>>()
        .iter(world)
        .map(|n| n.as_str().to_string())
        .collect()
}

fn last_line(app: &App, session: Entity) -> String {
    match app.world().get::<Transcript>(session).unwrap().last() {
        Some(Entry::Notice(text) | Entry::Error(text)) => {
            text.clone()
        }
        other => panic!("no notice: {other:?}"),
    }
}

#[test]
fn a_chief_removes_restores_and_reads_the_record_by_command() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app("decree", &rt, vec![]);
    with_librarian(&home);
    let names = residents;

    say(&mut app, "/remove librarian not needed any more");
    let session = settle(&mut app);
    assert!(last_line(&app, session).contains("Removed `librarian`"));
    assert!(names(&mut app).is_empty());
    assert_eq!(departed(&mut app), ["librarian"]);

    // A chief who speaks to it is told where it went.
    let direct = with_agent("librarian");
    app.world_mut().trigger(UserMessage {
        key: direct.clone(),
        user: "alice".into(),
        text: "Add Dune.".into(),
    });
    app.world_mut().flush();
    let asked = find(app.world_mut(), &direct).unwrap();
    assert!(last_line(&app, asked).contains("was removed"));

    say(&mut app, "/restore librarian it is needed after all");
    settle(&mut app);
    assert!(
        last_line(&app, session).contains("Restored `librarian`")
    );
    assert_eq!(names(&mut app), ["librarian"]);
    assert!(departed(&mut app).is_empty());

    say(&mut app, "/record");
    settle(&mut app);
    let record = last_line(&app, session);
    assert!(
        record.contains("removed `librarian`: not needed any more")
    );
    assert!(
        record.contains("restored `librarian`")
            && record.contains("it is needed after all")
    );
    // No model was asked for any of it.
    assert!(app.world().get::<Jobs>(session).is_none());

    say(&mut app, "/restore nobody");
    settle(&mut app);
    assert!(last_line(&app, session).contains("nothing is archived"));
    shutdown(app.world_mut());
}

#[test]
fn a_reply_to_a_told_agent_counts_as_a_message_of_the_same_task() {
    let rt = Runtime::new().unwrap();
    let tell = || {
        call(
            "tell_agent",
            json!({"name":"librarian","message":"Add Dune."}),
        )
    };
    let (mut app, home) = app(
        "tellcap",
        &rt,
        vec![
            tell(),
            says("ok"),
            says("ok"),
            // The mayor's turn on the reply tries to send again.
            tell(),
            says("Noted."),
        ],
    );
    limit(&home, "[task]\nmessages = 2\n");
    let hamlet = with_librarian(&home);
    say(&mut app, "have the librarian add Dune");

    let log = home.sessions().join("test_alice_main.jsonl");
    let refused = || {
        log::read_events(&log).unwrap_or_default().iter().any(|e| {
            e["type"] == "tool"
                && e["output"].as_str().is_some_and(|o| {
                    o.contains("used all 2 of its messages")
                })
        })
    };
    testing::until(
        &mut app,
        "the second message to be refused",
        |app| refused() && !testing::working(app, &with_mayor()),
    );
    // The message and its reply, under one task.
    let road = hamlet
        .roads()
        .into_iter()
        .find(|r| r.joins(MAYOR, "librarian"))
        .unwrap();
    assert_eq!(road.messages, 2);
    shutdown(app.world_mut());
}

#[test]
fn a_command_works_from_the_session_of_the_agent_it_is_about() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app("selected", &rt, vec![says("Here.")]);
    let hamlet = with_librarian(&home);
    hamlet
        .remove("librarian", "x", Who::Chief("alice"))
        .unwrap();
    app.update();
    assert_eq!(departed(&mut app).len(), 1);

    // The command line sends to the selected agent's session.
    let key = with_agent("librarian");
    let session = send(&mut app, &key, "/restore librarian");
    assert!(
        last_line(&app, session).contains("Restored `librarian`")
    );
    assert_eq!(residents(&mut app).len(), 1);
    assert!(departed(&mut app).is_empty());

    // And the agent answers there again.
    let session = send(&mut app, &key, "Are you there?");
    assert_eq!(
        app.world().get::<Transcript>(session).unwrap().last(),
        Some(&Entry::Assistant("Here.".into()))
    );
    shutdown(app.world_mut());
}

fn notices(app: &App, session: Entity) -> Vec<String> {
    app.world()
        .get::<Transcript>(session)
        .unwrap()
        .items()
        .iter()
        .filter_map(|i| match &i.entry {
            Entry::Notice(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_main_chat_follows_the_office_when_a_chief_replaces_the_mayor()
{
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "office",
        &rt,
        // The first mayor reflects as its worker is retired,
        // beside the new mayor's first turn.
        vec![
            says("I am the first mayor."),
            says("I am scout."),
            says("I am scout."),
        ],
    );
    let hamlet = Hamlet::new(home.root.clone());
    hamlet.create("scout", "Looks things up.", "test").unwrap();

    let session = send(&mut app, &with_mayor(), "who are you?");
    let seat = |app: &App| {
        app.world().get::<Seat>(session).unwrap().name.clone()
    };
    assert_eq!(seat(&app), FOUNDER);
    let first = agents::mayor(app.world_mut()).unwrap();
    assert_eq!(app.world().get::<TalksTo>(session).unwrap().0, first);

    send(&mut app, &with_mayor(), "/mayor scout it knows the way");
    assert_eq!(hamlet.mayor().name, "scout");
    assert_eq!(residents(&mut app), [FOUNDER]);
    let world = app.world_mut();
    let scout = agents::named(world, "scout").unwrap();
    assert_eq!(agents::mayor(world), Some(scout));
    assert_eq!(world.get::<TalksTo>(session).unwrap().0, scout);

    send(&mut app, &with_mayor(), "who are you?");
    assert_eq!(seat(&app), "scout");
    let t = app.world().get::<Transcript>(session).unwrap();
    assert_eq!(
        t.last(),
        Some(&Entry::Assistant("I am scout.".into()))
    );
    assert!(
        notices(&app, session)
            .iter()
            .any(|n| n.contains("`scout` holds this office now"))
    );
    // Each kept its own house and its own side of the chat.
    assert!(home.sessions().join("test_alice_main.jsonl").exists());
    assert!(
        home.root
            .join("agents/scout/sessions/test_alice_main.jsonl")
            .exists()
    );
    shutdown(app.world_mut());
}

#[test]
fn a_chief_founds_a_hamlet_and_reaches_its_mayor() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) =
        app("reach", &rt, vec![says("The mill is quiet.")]);
    let main =
        send(&mut app, &with_mayor(), "/found mill miller Grinds.");
    assert!(
        notices(&app, main)
            .iter()
            .any(|n| n.contains("`mill` is founded"))
    );
    let world = app.world_mut();
    let miller = agents::mayor_of(world, "mill").unwrap();
    let first = agents::mayor(world).unwrap();
    // Every mayor has a road to every other.
    assert!(
        world
            .query::<&agents::Road>()
            .iter(world)
            .any(|road| road.has(miller) && road.has(first))
    );

    let mill = SessionKey::with_mayor("test", "alice", "mill");
    let session = send(&mut app, &mill, "how is the mill?");
    let t = app.world().get::<Transcript>(session).unwrap();
    assert_eq!(
        t.last(),
        Some(&Entry::Assistant("The mill is quiet.".into()))
    );
    assert_eq!(
        app.world().get::<TalksTo>(session).unwrap().0,
        miller
    );
    assert!(
        home.root
            .join(
                "hamlets/mill/agents/miller/sessions/\
                 test_alice_mayor_mill.jsonl"
            )
            .exists()
    );
    // A command typed there is about that hamlet.
    send(&mut app, &mill, "/votes");
    assert!(
        notices(&app, session)
            .iter()
            .any(|n| n == "No votes in `mill` yet.")
    );
    // Nobody is there once it is removed.
    send(&mut app, &with_mayor(), "/remove-hamlet mill");
    let gone = send(&mut app, &mill, "hello?");
    let t = app.world().get::<Transcript>(gone).unwrap();
    assert!(matches!(t.last(), Some(Entry::Error(_))));
    assert!(agents::mayor_of(app.world_mut(), "mill").is_none());
    shutdown(app.world_mut());
}

#[test]
fn the_chief_is_told_of_a_vote_called_anywhere() {
    #[derive(Resource, Default)]
    struct Cast(Vec<Entity>);

    let rt = Runtime::new().unwrap();
    let call = |name: &str, args: Value| {
        json!({"role": "assistant", "content": "", "tool_calls": [{"id": "c1", "type": "function",
            "function": {"name": name, "arguments": args.to_string()}}]})
    };
    let ballot = json!({"choice": "no", "reason": "No grounds.", "evidence": ""});
    let (mut app, home) = app(
        "told",
        &rt,
        vec![
            call(
                "call_vote",
                json!({"kind": "found_hamlet", "hamlet": "mill", "mayor": "miller",
                       "purpose": "Grinds.", "reason": "We need flour."}),
            ),
            call("cast_ballot", ballot.clone()),
            call("cast_ballot", ballot.clone()),
            call("cast_ballot", ballot),
            says("It failed."),
        ],
    );
    app.init_resource::<Cast>().add_observer(
        |voted: On<agents::Voted>, mut cast: ResMut<Cast>| {
            cast.0.push(voted.agent)
        },
    );
    let hamlet = Hamlet::new(home.root.clone());
    for name in ["scout", "smith", "baker"] {
        hamlet.create(name, "Works.", "test").unwrap();
    }
    // The chief has the mayor's chat open and talks to an agent.
    let main = ensure(app.world_mut(), &with_mayor());
    let direct = with_agent("baker");
    send(&mut app, &direct, "call a vote for a mill");

    let told = notices(&app, main);
    assert!(told.iter().any(|n| {
        n.contains("`baker` called vote 1 in `home`")
            && n.contains("found a new hamlet `mill`")
    }));
    assert!(
        told.iter().any(|n| n.contains("Vote 1 in `home` failed"))
    );
    // Ballots are shown where the vote was called.
    assert!(!told.iter().any(|n| n.contains("voted")));
    let world = app.world_mut();
    let voters = ["scout", "smith", "baker"]
        .map(|name| agents::named(world, name).unwrap());
    assert_eq!(world.resource::<Cast>().0, voters);
    assert!(Realm::new(home.root.clone()).hamlet("mill").is_err());
    shutdown(app.world_mut());
}

#[test]
fn a_rewound_conversation_reads_the_same_live_and_from_its_log() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) =
        app("rewind", &rt, vec![says("1"), says("2"), says("3")]);
    say(&mut app, "one");
    settle(&mut app);
    say(&mut app, "two");
    let session = settle(&mut app);
    let live = |app: &App| {
        let said = app.world().get::<Transcript>(session).unwrap();
        said.items()
            .iter()
            .map(|item| item.entry.clone())
            .collect::<Vec<_>>()
    };
    let log = home.sessions().join("test_alice_main.jsonl");
    let read = || {
        Transcript::from_log(&log::read_standing(&log).unwrap())
            .items()
            .iter()
            .map(|item| item.entry.clone())
            .collect::<Vec<_>>()
    };
    app.world_mut().trigger(Rewind { session, entry: 2 });
    app.world_mut().flush();
    testing::until(&mut app, "the rewind", |app| {
        live(app).len() == 2
    });
    assert_eq!(live(&app), read());
    say(&mut app, "three");
    settle(&mut app);
    assert_eq!(live(&app), read());
    // No entry was given the id of one that went.
    let said = app.world().get::<Transcript>(session).unwrap();
    assert_eq!(said.ids(), [0, 1, 4, 5]);
    assert_eq!(said.get(5), Some(&Entry::Assistant("3".into())));
    shutdown(app.world_mut());
}
