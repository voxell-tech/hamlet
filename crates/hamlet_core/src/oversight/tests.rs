use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy_app::AppExit;
use hamlet_home::Home;
use hamlet_model::log;
use hamlet_rules::chief::asking::Pick;
use hamlet_rules::chief::rulings::{Choice, Party};
use hamlet_rules::hamlet::MAYOR;
use hamlet_sandbox::{Policy, Sandbox, Unsandboxed};
use hamlet_testing::Scratch;
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::runtime::Runtime;

use super::danger::Danger;
use super::*;
use crate::session::{self, Entry, SessionKey, Transcript};
use crate::testing::{
    self, ALICE, Place, bash, call, limit, place, say, says, settle,
    told, until, with_agent, with_mayor, working,
};

/// A sandbox that says it confines and does not, so a test can run
/// what a chief allowed.
struct Pretend;

impl Sandbox for Pretend {
    fn name(&self) -> &'static str {
        "pretend"
    }

    fn degraded(&self) -> bool {
        false
    }

    fn command(
        &self,
        policy: &Policy,
        argv: &[String],
        cwd: &Path,
    ) -> Command {
        Unsandboxed.command(policy, argv, cwd)
    }
}

/// The limits under which the mayor is asked about every
/// instruction.
const REVIEWED: &str = "[asking]\nmayor_review = \"always\"\n";

/// An app that keeps what waits for a chief.
fn app_in(
    home: &Place,
    rt: &Runtime,
    sandbox: &'static dyn Sandbox,
    replies: Vec<Value>,
) -> App {
    let mut app = testing::app(home, rt, sandbox, replies);
    app.add_plugins(OversightPlugin);
    app
}

/// An app whose sandbox confines, with the chief's limits as they
/// come: the model is on this machine, so the mayor reviews nothing.
fn app(tag: &str, rt: &Runtime, replies: Vec<Value>) -> (App, Place) {
    let home = place(&format!("oversight-{tag}"));
    let app = app_in(&home, rt, &Pretend, replies);
    (app, home)
}

fn answer(app: &mut App, entity: Entity, choice: Choice, text: &str) {
    app.world_mut().trigger(Answer {
        entity,
        choice,
        text: text.into(),
        chief: ALICE.into(),
    });
    app.world_mut().flush();
}

/// Everything that waits, oldest first.
fn waits(world: &mut World) -> Vec<Need> {
    let mut rows = world
        .query::<(Entity, &Waiting, Option<&Asks>)>()
        .iter(world)
        .collect::<Vec<_>>();
    rows.sort_by_key(|(_, waiting, _)| waiting.order);
    rows.into_iter()
        .map(|(entity, waiting, asks)| {
            Need::of(entity, waiting, asks)
        })
        .collect()
}

fn queue(app: &mut App) -> Vec<Need> {
    app.update();
    waits(app.world_mut())
}

/// Ticks until exactly `n` things wait, and returns them.
fn waiting(app: &mut App, n: usize) -> Vec<Need> {
    until(app, "the queue to fill", |app| {
        waits(app.world_mut()).len() == n
    });
    queue(app)
}

fn mayor_log(home: &Home) -> PathBuf {
    home.sessions().join("test_alice_main.jsonl")
}

fn librarian_log(home: &Home) -> PathBuf {
    home.root.join(
        "agents/librarian/sessions/test_alice_agent_librarian.jsonl",
    )
}

fn with_librarian(home: &Home) {
    Hamlet::new(home.root.clone())
        .create("librarian", "Keeps the reading list.", "test")
        .unwrap();
}

#[test]
fn a_network_command_waits_for_the_chief_and_runs_once_allowed() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "allow",
        &rt,
        vec![
            bash("echo https://example.org > fetched.txt"),
            bash("echo local > plain.txt"),
            says("done"),
        ],
    );
    say(&mut app, &with_mayor(), "fetch it");
    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].kind, Kind::Approval(Danger::Network));
    assert_eq!(needs[0].by, MAYOR);
    assert_eq!(
        needs[0].about,
        "echo https://example.org > fetched.txt"
    );
    assert_eq!(
        needs[0].answers,
        vec![Choice::Allow, Choice::Always, Choice::Deny]
    );
    // It hangs on the agent that raised it.
    let raiser = crate::agents::mayor(app.world_mut()).unwrap();
    assert_eq!(
        app.world().get::<RaisedBy>(needs[0].entity).map(|r| r.0),
        Some(raiser)
    );
    let hung = app.world().get::<Needs>(raiser).unwrap();
    assert_eq!(hung.iter().collect::<Vec<_>>(), [needs[0].entity]);
    // Held, not run, and the turn is still open.
    for _ in 0..20 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!home.workspace.join("fetched.txt").exists());
    assert!(working(&mut app, &with_mayor()));

    // An answer the thing does not offer changes nothing.
    answer(&mut app, needs[0].entity, Choice::GoAhead, "");
    assert_eq!(queue(&mut app).len(), 1);

    answer(&mut app, needs[0].entity, Choice::Allow, "");
    settle(&mut app, &with_mayor());
    assert!(home.workspace.join("fetched.txt").exists());
    // A command that stays inside its house never asked.
    assert!(home.workspace.join("plain.txt").exists());
    assert!(queue(&mut app).is_empty());
    assert!(app.world().get::<Needs>(raiser).is_none());

    let record = Ledger::open(&home.root).events();
    assert_eq!(record[0]["type"], "raised");
    assert_eq!(record[1]["how"], "allow");
    assert_eq!(record[1]["chief"], "alice");
    session::shutdown(app.world_mut());
}

#[test]
fn a_refusal_reaches_the_agent_and_covers_a_reworded_command() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "deny",
        &rt,
        vec![
            bash("curl -s https://example.org > a.txt"),
            bash("wget -q https://example.org -O b.txt"),
            says("I could not fetch it."),
            // The next turn may ask again.
            bash("curl -s https://example.org > c.txt"),
            says("ok"),
        ],
    );
    say(&mut app, &with_mayor(), "fetch it");
    let needs = waiting(&mut app, 1);
    answer(
        &mut app,
        needs[0].entity,
        Choice::Deny,
        "not on this machine",
    );
    settle(&mut app, &with_mayor());
    assert!(queue(&mut app).is_empty());
    let told = told(&mayor_log(&home));
    assert!(
        told[0].contains("the chief refused it: not on this machine"),
        "{told:?}"
    );
    assert!(told[1].contains("already refused"), "{told:?}");
    assert!(!home.workspace.join("a.txt").exists());
    assert!(!home.workspace.join("b.txt").exists());

    say(&mut app, &with_mayor(), "try once more");
    let needs = waiting(&mut app, 1);
    answer(&mut app, needs[0].entity, Choice::Deny, "");
    settle(&mut app, &with_mayor());
    session::shutdown(app.world_mut());
}

#[test]
fn always_gives_the_agent_the_permission_until_it_is_revoked() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "always",
        &rt,
        vec![
            bash("echo https://a.test > a.txt"),
            says("one"),
            bash("echo https://b.test > b.txt"),
            says("two"),
            bash("echo https://c.test > c.txt"),
            says("three"),
        ],
    );
    say(&mut app, &with_mayor(), "fetch a");
    let needs = waiting(&mut app, 1);
    answer(&mut app, needs[0].entity, Choice::Always, "");
    settle(&mut app, &with_mayor());
    assert!(home.workspace.join("a.txt").exists());
    assert!(Grants::open(&home.root).holds(0, Permission::Network));

    // The like of it does not ask.
    say(&mut app, &with_mayor(), "fetch b");
    settle(&mut app, &with_mayor());
    assert!(home.workspace.join("b.txt").exists());
    assert!(queue(&mut app).is_empty());

    app.world_mut().trigger(Revoke {
        agent: MAYOR.into(),
        permission: Permission::Network,
        chief: "alice".into(),
    });
    say(&mut app, &with_mayor(), "fetch c");
    let needs = waiting(&mut app, 1);
    assert!(!home.workspace.join("c.txt").exists());
    answer(&mut app, needs[0].entity, Choice::Deny, "");
    settle(&mut app, &with_mayor());
    session::shutdown(app.world_mut());
}

#[test]
fn with_no_sandbox_a_command_waits_and_a_destructive_one_always_does()
{
    let rt = Runtime::new().unwrap();
    let home = place("oversight-bare");
    let mut app = app_in(
        &home,
        &rt,
        &Unsandboxed,
        vec![
            bash("echo hi > a.txt"),
            // Never run: the test refuses it.
            bash("rm -rf ~/hamlet-oversight-never"),
            says("done"),
        ],
    );
    say(&mut app, &with_mayor(), "go");
    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].kind, Kind::Approval(Danger::Unconfined));
    answer(&mut app, needs[0].entity, Choice::Always, "");

    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].kind, Kind::Approval(Danger::Destructive));
    // Nothing can make this one stop asking.
    assert_eq!(needs[0].answers, vec![Choice::Allow, Choice::Deny]);
    answer(&mut app, needs[0].entity, Choice::Always, "");
    assert_eq!(queue(&mut app).len(), 1);
    answer(&mut app, needs[0].entity, Choice::Deny, "no");
    settle(&mut app, &with_mayor());
    assert!(home.workspace.join("a.txt").exists());
    session::shutdown(app.world_mut());
}

#[test]
fn three_refusals_in_a_turn_end_the_asking() {
    let rt = Runtime::new().unwrap();
    let home = place("oversight-strikes");
    let mut app = app_in(
        &home,
        &rt,
        &Unsandboxed,
        vec![
            bash("sudo true 1"),
            bash("sudo true 2"),
            bash("sudo true 3"),
            bash("sudo true 4"),
            says("I give up."),
        ],
    );
    say(&mut app, &with_mayor(), "go");
    for n in 1..=3 {
        let needs = waiting(&mut app, 1);
        let asked = format!("sudo true {n}");
        assert_eq!(needs[0].about, asked);
        answer(&mut app, needs[0].entity, Choice::Deny, "no");
        until(&mut app, "the refusal to land", |app| {
            let needs = waits(app.world_mut());
            needs.iter().all(|need| need.about != asked)
        });
    }
    settle(&mut app, &with_mayor());
    assert!(queue(&mut app).is_empty());
    let told = told(&mayor_log(&home));
    assert!(told[3].contains("refused 3 requests"), "{told:?}");
    session::shutdown(app.world_mut());
}

#[test]
fn the_same_call_from_two_sessions_waits_as_one() {
    let rt = Runtime::new().unwrap();
    let command = "echo https://example.org >> twice.txt";
    let (mut app, home) = app(
        "twice",
        &rt,
        vec![bash(command), bash(command), says("ok"), says("ok")],
    );
    let other = SessionKey {
        thread: "second".into(),
        ..with_mayor()
    };
    say(&mut app, &with_mayor(), "fetch");
    say(&mut app, &other, "fetch");
    until(&mut app, "both to ask", |app| {
        let needs = waits(app.world_mut());
        needs.len() == 1 && needs[0].asked == 2
    });
    let needs = queue(&mut app);
    answer(&mut app, needs[0].entity, Choice::Allow, "");
    settle(&mut app, &with_mayor());
    settle(&mut app, &other);
    let text =
        std::fs::read_to_string(home.workspace.join("twice.txt"))
            .unwrap();
    assert_eq!(text.lines().count(), 2);
    session::shutdown(app.world_mut());
}

#[test]
fn quitting_with_something_waiting_approves_nothing() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "quit",
        &rt,
        vec![bash("echo https://example.org > fetched.txt")],
    );
    say(&mut app, &with_mayor(), "fetch it");
    waiting(&mut app, 1);
    let started = Instant::now();
    app.world_mut().write_message(AppExit::Success);
    app.update();
    assert!(started.elapsed() < Duration::from_secs(2));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!home.workspace.join("fetched.txt").exists());
    drop(app);

    // The next start tells the chief, and has allowed nothing.
    let mut app = app_in(&home, &rt, &Pretend, vec![]);
    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].kind, Kind::Lapsed);
    assert_eq!(needs[0].answers, vec![Choice::Dismiss]);
    assert!(needs[0].about.contains("was not carried out"));
    assert!(needs[0].about.contains("echo https://example.org"));
    assert!(Grants::open(&home.root).held().is_empty());
    assert!(!home.workspace.join("fetched.txt").exists());
    answer(&mut app, needs[0].entity, Choice::Dismiss, "");
    assert!(queue(&mut app).is_empty());
    // Once told, it is not raised a second time.
    drop(app);
    let mut app = app_in(&home, &rt, &Pretend, vec![]);
    assert!(queue(&mut app).is_empty());
    rt.shutdown_background();
}

#[test]
fn an_agent_asked_by_another_is_held_under_its_own_name() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "nested",
        &rt,
        vec![
            call(
                "ask_agent",
                json!({"name": "librarian", "message": "Fetch the list."}),
            ),
            bash("echo https://example.org > list.txt"),
            says("Fetched."),
            says("The librarian has it."),
        ],
    );
    with_librarian(&home);
    say(&mut app, &with_mayor(), "get the list");
    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].by, "librarian");
    // The mayor's turn waits with it.
    assert!(working(&mut app, &with_mayor()));
    answer(&mut app, needs[0].entity, Choice::Always, "");
    settle(&mut app, &with_mayor());
    let house = home.root.join("agents/librarian/workspace");
    assert!(house.join("list.txt").exists());
    // The permission is the librarian's, not the mayor's.
    let grants = Grants::open(&home.root);
    let number = Hamlet::new(home.root.clone())
        .member("librarian")
        .unwrap()
        .id;
    assert!(grants.holds(number, Permission::Network));
    assert!(!grants.holds(0, Permission::Network));
    // Waiting cost the task no more than its one message.
    let hamlet = Hamlet::new(home.root.clone());
    assert_eq!(hamlet.roads()[0].messages, 1);
    session::shutdown(app.world_mut());
}

#[test]
fn an_agent_that_leaves_takes_what_it_waited_for_with_it() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "gone",
        &rt,
        vec![
            call(
                "ask_agent",
                json!({"name": "librarian", "message": "Fetch the list."}),
            ),
            bash("echo https://example.org > list.txt"),
            says("I could not."),
            says("It could not."),
        ],
    );
    with_librarian(&home);
    let hamlet = Hamlet::new(home.root.clone());
    let number = hamlet.member("librarian").unwrap().id;
    app.world_mut()
        .resource_mut::<Grants>()
        .give(number, "librarian", Permission::Unconfined, "alice")
        .unwrap();
    say(&mut app, &with_mayor(), "get the list");
    waiting(&mut app, 1);
    // The chief removes it while its request waits.
    let other = SessionKey {
        thread: "second".into(),
        ..with_mayor()
    };
    say(&mut app, &other, "/remove librarian not needed");
    settle(&mut app, &with_mayor());
    assert!(queue(&mut app).is_empty());
    let house = home
        .root
        .join("archive")
        .join(&hamlet.archived()[0].id)
        .join("house");
    assert!(!house.join("workspace/list.txt").exists());
    let told = told(&house.join("sessions/mayor.jsonl"));
    assert!(
        told[0].contains("nobody is here to approve"),
        "{told:?}"
    );
    // Its grants went with it, and do not come back.
    assert!(Grants::open(&home.root).held().is_empty());
    say(&mut app, &other, "/restore librarian");
    app.update();
    assert!(Grants::open(&home.root).held().is_empty());
    session::shutdown(app.world_mut());
}

#[test]
fn an_agent_objects_once_and_the_work_is_held_until_the_chief_replies()
 {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "object",
        &rt,
        vec![
            call(
                "object",
                json!({"reason": "The list is the only copy."}),
            ),
            call("object", json!({"reason": "I still think so."})),
            bash("rm -f list.txt; echo gone > done.txt"),
            says("Deleted."),
        ],
    );
    with_librarian(&home);
    say(
        &mut app,
        &with_agent("librarian"),
        "Delete the reading list.",
    );
    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].kind, Kind::Objection(Party::Agent));
    assert_eq!(needs[0].by, "librarian");
    assert!(needs[0].about.contains("alice told librarian: Delete"));
    assert!(needs[0].about.contains("The list is the only copy."));
    assert_eq!(
        needs[0].answers,
        vec![Choice::GoAhead, Choice::Withdraw]
    );
    for _ in 0..20 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(working(&mut app, &with_agent("librarian")));

    answer(
        &mut app,
        needs[0].entity,
        Choice::GoAhead,
        "I have a backup",
    );
    settle(&mut app, &with_agent("librarian"));
    assert!(queue(&mut app).is_empty());
    let house = home.root.join("agents/librarian/workspace");
    assert!(house.join("done.txt").exists());
    let told = told(&librarian_log(&home));
    assert!(
        told[0].contains("The chief says go ahead: I have a backup"),
        "{told:?}"
    );
    // The second objection was refused by the loop, not by the model.
    assert!(told[1].contains("may not object"), "{told:?}");
    session::shutdown(app.world_mut());
}

#[test]
fn a_withdrawn_instruction_stops_the_work() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "withdraw",
        &rt,
        vec![
            call(
                "object",
                json!({"reason": "The list is the only copy."}),
            ),
            // Never reached.
            bash("echo gone > done.txt"),
            says("Deleted."),
        ],
    );
    with_librarian(&home);
    say(
        &mut app,
        &with_agent("librarian"),
        "Delete the reading list.",
    );
    let needs = waiting(&mut app, 1);
    answer(
        &mut app,
        needs[0].entity,
        Choice::Withdraw,
        "you are right",
    );
    settle(&mut app, &with_agent("librarian"));
    assert!(queue(&mut app).is_empty());
    let house = home.root.join("agents/librarian");
    assert!(!house.join("workspace/done.txt").exists());
    let events = log::read_events(&librarian_log(&home)).unwrap();
    let last = events.last().unwrap();
    assert_eq!(last["type"], "turn_aborted");
    assert!(
        last["reason"]
            .as_str()
            .unwrap()
            .contains("withdrew the instruction: you are right")
    );
    session::shutdown(app.world_mut());
}

#[test]
fn only_the_instructed_agent_may_object() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "stranger",
        &rt,
        vec![
            call(
                "ask_agent",
                json!({"name": "librarian", "message": "Delete the list."}),
            ),
            // The librarian was asked by the mayor, not by a chief.
            call("object", json!({"reason": "No."})),
            says("Fine."),
            says("Done."),
        ],
    );
    with_librarian(&home);
    say(&mut app, &with_mayor(), "have the list deleted");
    settle(&mut app, &with_mayor());
    assert!(queue(&mut app).is_empty());
    let told = told(
        &home.root.join("agents/librarian/sessions/mayor.jsonl"),
    );
    assert!(told[0].contains("may not object"), "{told:?}");
    session::shutdown(app.world_mut());
}

#[test]
fn the_mayor_objects_beside_the_work_and_holds_its_next_step() {
    let rt = Runtime::new().unwrap();
    let home = place("oversight-mayor");
    limit(&home, REVIEWED);
    let mut app = app_in(
        &home,
        &rt,
        &Pretend,
        vec![
            // The work starts without waiting for the mayor.
            bash("echo one > a.txt; sleep 1"),
            // The mayor's review, asked once the work had begun.
            call(
                "object",
                json!({"reason": "The scout needs that list."}),
            ),
            bash("echo two > b.txt"),
            says("Deleted."),
        ],
    );
    with_librarian(&home);
    say(
        &mut app,
        &with_agent("librarian"),
        "Delete the reading list.",
    );
    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].kind, Kind::Objection(Party::Mayor));
    assert_eq!(needs[0].by, MAYOR);
    assert!(needs[0].about.contains("The scout needs that list."));
    let house = home.root.join("agents/librarian/workspace");
    std::thread::sleep(Duration::from_millis(1300));
    for _ in 0..20 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(house.join("a.txt").exists());
    assert!(!house.join("b.txt").exists());
    assert!(working(&mut app, &with_agent("librarian")));

    answer(&mut app, needs[0].entity, Choice::GoAhead, "");
    settle(&mut app, &with_agent("librarian"));
    assert!(house.join("b.txt").exists());
    assert!(queue(&mut app).is_empty());
    // The review was one model call, kept where it is counted.
    let review = log::read_events(
        &home.sessions().join("hamlet_review_main.jsonl"),
    )
    .unwrap();
    assert_eq!(
        review.iter().filter(|e| e["type"] == "model_call").count(),
        1
    );
    session::shutdown(app.world_mut());
}

#[test]
fn an_agent_asks_the_chief_a_question_and_waits_for_the_reply() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "question",
        &rt,
        vec![
            call("ask_chief", json!({"question": "Paper or ebook?"})),
            call("ask_chief", json!({"question": "Hardback?"})),
            says("Paper it is."),
        ],
    );
    limit(&home, "[asking]\nquestions = 1\n");
    say(&mut app, &with_mayor(), "order Dune");
    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].kind, Kind::Question);
    assert_eq!(needs[0].about, "Paper or ebook?");
    answer(&mut app, needs[0].entity, Choice::Reply, "paper");
    settle(&mut app, &with_mayor());
    assert!(queue(&mut app).is_empty());
    let told = told(&mayor_log(&home));
    assert_eq!(told[0], "The chief answers: paper");
    assert!(told[1].contains("asked the chief once"), "{told:?}");
    session::shutdown(app.world_mut());
}

#[test]
fn an_agent_asks_several_questions_and_hears_each_answer() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "questions",
        &rt,
        vec![
            call(
                "ask_chief",
                json!({"questions": [
                    {"question": "Paper or ebook?",
                     "options": [{"title": "Paper", "description": "Ships"}, "Ebook"]},
                    {"question": "Budget?"}
                ]}),
            ),
            says("Paper, under 30."),
        ],
    );
    say(&mut app, &with_mayor(), "order Dune");
    let needs = waiting(&mut app, 1);
    let asked = needs[0].questions.clone().unwrap();
    assert_eq!(asked.questions.len(), 2);
    assert!(needs[0].about.contains("a) Paper - Ships"));
    let entity = needs[0].entity;
    // Answers that leave a question out answer nothing.
    app.world_mut().trigger(Answered {
        entity,
        picks: vec![Pick::Offer(0)],
        chief: ALICE.into(),
    });
    app.world_mut().flush();
    assert_eq!(queue(&mut app).len(), 1);
    app.world_mut().trigger(Answered {
        entity,
        picks: vec![Pick::Offer(0), Pick::Words("under 30".into())],
        chief: ALICE.into(),
    });
    app.world_mut().flush();
    settle(&mut app, &with_mayor());
    assert!(queue(&mut app).is_empty());
    assert_eq!(
        told(&mayor_log(&home))[0],
        "The chief answers:\n> Paper or ebook?\nPaper\n\n> \
         Budget?\nunder 30"
    );
    session::shutdown(app.world_mut());
}

#[test]
fn a_chief_stops_everything_and_the_next_turn_is_still_gated() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "stop",
        &rt,
        vec![
            bash("sleep 30"),
            bash("echo https://example.org > after.txt"),
            says("ok"),
        ],
    );
    say(&mut app, &with_mayor(), "wait a long time");
    until(&mut app, "the command to start", |_| {
        log::read_events(&mayor_log(&home))
            .unwrap()
            .iter()
            .any(|e| e["type"] == "assistant")
    });
    say(&mut app, &with_mayor(), "and then this");
    let started = Instant::now();
    app.world_mut().trigger(Veto {
        agent: None,
        chief: "alice".into(),
    });
    app.world_mut().flush();
    settle(&mut app, &with_mayor());
    assert!(started.elapsed() < Duration::from_secs(3));
    let events = log::read_events(&mayor_log(&home)).unwrap();
    assert_eq!(events.last().unwrap()["type"], "turn_aborted");
    // The message queued behind it was dropped, not run.
    assert_eq!(
        events.iter().filter(|e| e["type"] == "user").count(),
        1
    );
    // And its instruction went with it.
    let world = app.world_mut();
    let carried = world.query::<&Instruction>().iter(world).count();
    assert_eq!(carried, 0);

    say(&mut app, &with_mayor(), "now fetch");
    let needs = waiting(&mut app, 1);
    assert!(!home.workspace.join("after.txt").exists());
    answer(&mut app, needs[0].entity, Choice::Deny, "");
    settle(&mut app, &with_mayor());
    session::shutdown(app.world_mut());
}

#[test]
fn a_chief_stops_one_agent_wherever_it_is_working() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "stop-one",
        &rt,
        vec![
            call(
                "ask_agent",
                json!({"name": "librarian", "message": "Sort the list."}),
            ),
            bash("sleep 30"),
            // After the stop: the gate must have survived it.
            bash("echo https://example.org > after.txt"),
            says("ok"),
        ],
    );
    with_librarian(&home);
    say(&mut app, &with_mayor(), "have the list sorted");
    let house =
        home.root.join("agents/librarian/sessions/mayor.jsonl");
    until(&mut app, "the librarian to start", |_| {
        log::read_events(&house)
            .unwrap()
            .iter()
            .any(|e| e["type"] == "assistant")
    });
    // Stopping someone else stops nothing.
    app.world_mut().trigger(Veto {
        agent: Some("scout".into()),
        chief: "alice".into(),
    });
    app.world_mut().flush();
    for _ in 0..10 {
        app.update();
    }
    assert!(working(&mut app, &with_mayor()));

    let started = Instant::now();
    app.world_mut().trigger(Veto {
        agent: Some("librarian".into()),
        chief: "alice".into(),
    });
    app.world_mut().flush();
    settle(&mut app, &with_mayor());
    assert!(started.elapsed() < Duration::from_secs(3));

    say(&mut app, &with_mayor(), "now fetch");
    let needs = waiting(&mut app, 1);
    answer(&mut app, needs[0].entity, Choice::Deny, "");
    settle(&mut app, &with_mayor());
    assert!(!home.workspace.join("after.txt").exists());
    session::shutdown(app.world_mut());
}

/// The window's STOP names the agent it shows, and a mayor's own name
/// is not the office.
#[test]
fn a_stop_by_the_name_the_window_shows_stops_the_mayor() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "stop-shown-name",
        &rt,
        vec![bash("sleep 30"), says("ok")],
    );
    say(&mut app, &with_mayor(), "wait a long time");
    until(&mut app, "the command to start", |_| {
        log::read_events(&mayor_log(&home))
            .unwrap()
            .iter()
            .any(|e| e["type"] == "assistant")
    });
    let whose = crate::agents::mayor(app.world_mut()).unwrap();
    let shown = crate::agents::name_of(app.world(), whose);
    assert_eq!(shown, hamlet_rules::hamlet::FOUNDER);
    let started = Instant::now();
    app.world_mut().trigger(Veto {
        agent: Some(shown),
        chief: "alice".into(),
    });
    app.world_mut().flush();
    for _ in 0..20 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !working(&mut app, &with_mayor()),
        "the turn went on running"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    session::shutdown(app.world_mut());
}

#[test]
fn stopping_a_turn_drops_what_it_was_waiting_for() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "stop-held",
        &rt,
        vec![bash("echo https://example.org > fetched.txt")],
    );
    say(&mut app, &with_mayor(), "fetch it");
    waiting(&mut app, 1);
    app.world_mut().trigger(Veto {
        agent: Some(MAYOR.into()),
        chief: "alice".into(),
    });
    app.world_mut().flush();
    settle(&mut app, &with_mayor());
    assert!(queue(&mut app).is_empty());
    assert!(!home.workspace.join("fetched.txt").exists());
    let record = Ledger::open(&home.root).events();
    assert_eq!(record.last().unwrap()["how"], "stopped");
    session::shutdown(app.world_mut());
}

#[test]
fn a_turn_that_fails_is_a_fault_until_the_chief_has_seen_it() {
    let rt = Runtime::new().unwrap();
    // Nothing to replay, so the model call fails.
    let (mut app, _home) = app("fault", &rt, vec![]);
    say(&mut app, &with_mayor(), "hello");
    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].kind, Kind::Fault);
    assert!(needs[0].about.contains("replay exhausted"));
    // The same fault again is the same thing, counted.
    say(&mut app, &with_mayor(), "hello?");
    until(&mut app, "the second fault", |app| {
        waits(app.world_mut())[0].asked == 2
    });
    let needs = queue(&mut app);
    assert_eq!(needs.len(), 1);
    answer(&mut app, needs[0].entity, Choice::Dismiss, "");
    assert!(queue(&mut app).is_empty());
    session::shutdown(app.world_mut());
}

#[test]
fn the_queue_is_oldest_first() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "order",
        &rt,
        vec![
            call("ask_chief", json!({"question": "First?"})),
            call("ask_chief", json!({"question": "Second?"})),
            says("ok"),
            says("ok"),
        ],
    );
    with_librarian(&home);
    let other = with_agent("librarian");
    say(&mut app, &with_mayor(), "one");
    waiting(&mut app, 1);
    say(&mut app, &other, "two");
    let needs = waiting(&mut app, 2);
    assert_eq!(needs[0].about, "First?");
    assert_eq!(needs[1].about, "Second?");
    assert_eq!(needs[0].waited, "under a minute");
    answer(&mut app, needs[1].entity, Choice::Dismiss, "");
    answer(&mut app, needs[0].entity, Choice::Reply, "yes");
    settle(&mut app, &with_mayor());
    settle(&mut app, &other);
    session::shutdown(app.world_mut());
}

/// The latest notice in the conversation `key`.
fn notice(app: &mut App, key: &SessionKey) -> String {
    let world = app.world_mut();
    world
        .query::<(&SessionKey, &Transcript)>()
        .iter(world)
        .find(|(k, _)| *k == key)
        .and_then(|(_, t)| match t.last() {
            Some(Entry::Notice(text)) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn users(path: PathBuf) -> usize {
    log::read_events(&path)
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "user")
        .count()
}

#[test]
fn what_the_limits_reserve_waits_for_the_chief_to_allow_or_deny() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "reserved",
        &rt,
        vec![
            call(
                "create_agent",
                json!({"name": "scout", "purpose": "Looks things up.", "reason": "asked"}),
            ),
            call(
                "create_agent",
                json!({"name": "clerk", "purpose": "Files things.", "reason": "tidy"}),
            ),
            says("One of two."),
        ],
    );
    limit(&home, "[mayor]\ncreate_agents = false\n");
    say(&mut app, &with_mayor(), "grow the hamlet");
    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].kind, Kind::Approval(Danger::Reserved));
    assert_eq!(needs[0].by, MAYOR);
    assert!(needs[0].about.starts_with("create_agent "));
    assert!(needs[0].about.contains("scout"));
    // Only the limits file can make this stop asking.
    assert_eq!(needs[0].answers, vec![Choice::Allow, Choice::Deny]);
    let hamlet = Hamlet::new(home.root.clone());
    assert!(hamlet.residents().is_empty());
    answer(&mut app, needs[0].entity, Choice::Allow, "");

    let needs = waiting(&mut app, 1);
    assert!(needs[0].about.contains("clerk"));
    assert_eq!(hamlet.residents()[0].name, "scout");
    answer(&mut app, needs[0].entity, Choice::Deny, "one is enough");
    settle(&mut app, &with_mayor());
    assert_eq!(hamlet.residents().len(), 1);
    let told = told(&mayor_log(&home));
    assert!(told[0].starts_with("Created `scout`"), "{told:?}");
    assert!(
        told[1].contains("the chief refused it: one is enough"),
        "{told:?}"
    );
    session::shutdown(app.world_mut());
}

#[test]
fn the_limits_give_a_permission_by_name() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "standing",
        &rt,
        vec![bash("echo https://example.org > a.txt"), says("done")],
    );
    limit(&home, "[permissions]\nnetwork = [\"mayor\"]\n");
    say(&mut app, &with_mayor(), "fetch");
    settle(&mut app, &with_mayor());
    assert!(home.workspace.join("a.txt").exists());
    assert!(Ledger::open(&home.root).events().is_empty());
    session::shutdown(app.world_mut());
}

#[test]
fn a_review_that_passes_holds_nothing_and_is_charged_to_the_task() {
    let rt = Runtime::new().unwrap();
    let home = place("oversight-charged");
    limit(&home, REVIEWED);
    let mut app = app_in(
        &home,
        &rt,
        &Pretend,
        vec![bash("sleep 1"), says("pass"), says("Added.")],
    );
    with_librarian(&home);
    say(&mut app, &with_agent("librarian"), "Add Dune.");
    settle(&mut app, &with_agent("librarian"));
    // A mayor that passes holds nothing.
    assert!(queue(&mut app).is_empty());
    let spent = hamlet_rules::budget::Ledger::new(&home.root)
        .since(hamlet_rules::budget::today());
    let task = |agent: &str| {
        spent
            .iter()
            .filter(|(_, line)| line["agent"] == agent)
            .map(|(_, line)| {
                line["task"].as_str().unwrap().to_string()
            })
            .collect::<Vec<_>>()
    };
    // One call of the mayor's, under the task the agent worked on.
    assert_eq!(task(MAYOR).len(), 1);
    assert_eq!(task("librarian").len(), 2);
    assert_eq!(task(MAYOR)[0], task("librarian")[0]);
    session::shutdown(app.world_mut());
}

#[test]
fn a_spent_budget_buys_no_review() {
    let rt = Runtime::new().unwrap();
    let home = place("oversight-unpaid");
    let mut app = app_in(
        &home,
        &rt,
        &Pretend,
        // The agent's one call spends the budget.
        vec![bash("true"), says("never asked for")],
    );
    limit(
        &home,
        "[asking]\nmayor_review = \"always\"\n[budget]\ncost = \
         0.000001\n",
    );
    with_librarian(&home);
    say(&mut app, &with_agent("librarian"), "Add Dune.");
    settle(&mut app, &with_agent("librarian"));
    for _ in 0..20 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !home.sessions().join("hamlet_review_main.jsonl").exists()
    );
    let spent = hamlet_rules::budget::Ledger::new(&home.root)
        .since(hamlet_rules::budget::today());
    assert!(spent.iter().all(|(_, line)| line["agent"] != MAYOR));
    session::shutdown(app.world_mut());
}

#[test]
fn a_line_typed_to_an_agent_answers_its_question() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "typed-reply",
        &rt,
        vec![
            call("ask_chief", json!({"question": "Paper or ebook?"})),
            says("Paper it is."),
        ],
    );
    say(&mut app, &with_mayor(), "order Dune");
    waiting(&mut app, 1);
    say(&mut app, &with_mayor(), "paper");
    settle(&mut app, &with_mayor());
    assert!(queue(&mut app).is_empty());
    assert_eq!(
        told(&mayor_log(&home))[0],
        "The chief answers: paper"
    );
    // The line was the answer, not a second message.
    assert_eq!(users(mayor_log(&home)), 1);
    session::shutdown(app.world_mut());
}

#[test]
fn a_line_typed_to_an_objecting_agent_is_held_back_and_a_command_answers()
 {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "typed-hold",
        &rt,
        vec![
            call(
                "object",
                json!({"reason": "The list is the only copy."}),
            ),
            says("Deleted."),
        ],
    );
    with_librarian(&home);
    say(
        &mut app,
        &with_agent("librarian"),
        "Delete the reading list.",
    );
    waiting(&mut app, 1);
    // Words alone never count as a go-ahead.
    say(&mut app, &with_agent("librarian"), "no wait, you are right");
    let said = notice(&mut app, &with_agent("librarian"));
    assert!(said.starts_with("Not sent."), "{said}");
    assert!(said.contains("/go /withdraw"), "{said}");
    assert_eq!(queue(&mut app).len(), 1);
    assert!(working(&mut app, &with_agent("librarian")));

    // An answer that does not fit is explained, not guessed at.
    say(&mut app, &with_agent("librarian"), "/allow");
    assert!(
        notice(&mut app, &with_agent("librarian"))
            .starts_with("That cannot be answered that way")
    );
    say(&mut app, &with_agent("librarian"), "/go I have a backup");
    settle(&mut app, &with_agent("librarian"));
    assert!(queue(&mut app).is_empty());
    assert!(
        told(&librarian_log(&home))[0]
            .contains("go ahead: I have a backup")
    );
    assert_eq!(users(librarian_log(&home)), 1);
    session::shutdown(app.world_mut());
}

#[test]
fn a_command_typed_anywhere_answers_the_oldest_thing() {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "typed-queue",
        &rt,
        vec![bash("echo https://example.org > a.txt"), says("done")],
    );
    with_librarian(&home);
    say(&mut app, &with_mayor(), "fetch");
    waiting(&mut app, 1);
    // The librarian is selected; nothing of its own waits.
    say(&mut app, &with_agent("librarian"), "/queue");
    assert!(
        notice(&mut app, &with_agent("librarian"))
            .contains("[approval] mayor")
    );
    say(&mut app, &with_agent("librarian"), "/allow");
    settle(&mut app, &with_mayor());
    assert!(home.workspace.join("a.txt").exists());
    say(&mut app, &with_agent("librarian"), "/queue");
    assert_eq!(
        notice(&mut app, &with_agent("librarian")),
        "Nothing waits for you."
    );
    // No model was asked on the librarian's behalf.
    assert!(!librarian_log(&home).exists());
    session::shutdown(app.world_mut());
}

#[test]
fn an_objection_keeps_the_agent_safe_from_the_mayor_until_it_is_answered()
 {
    let rt = Runtime::new().unwrap();
    let (mut app, home) = app(
        "protected",
        &rt,
        vec![
            call(
                "object",
                json!({"reason": "The list is the only copy."}),
            ),
            // The mayor, meanwhile, tries to be rid of it.
            call(
                "remove_agent",
                json!({"name": "librarian", "reason": "It argues."}),
            ),
            says("I could not."),
            says("Deleted."),
            call(
                "remove_agent",
                json!({"name": "librarian", "reason": "No longer needed."}),
            ),
            says("Removed."),
        ],
    );
    with_librarian(&home);
    say(
        &mut app,
        &with_agent("librarian"),
        "Delete the reading list.",
    );
    waiting(&mut app, 1);
    say(&mut app, &with_mayor(), "remove the librarian");
    settle(&mut app, &with_mayor());
    let hamlet = Hamlet::new(home.root.clone());
    assert_eq!(hamlet.residents().len(), 1);
    let refused = told(&mayor_log(&home));
    assert!(
        refused[0].contains("to avoid a chief's task"),
        "{refused:?}"
    );

    let needs = queue(&mut app);
    answer(&mut app, needs[0].entity, Choice::GoAhead, "");
    settle(&mut app, &with_agent("librarian"));
    // The task is done and no timer stands in for the objection.
    say(&mut app, &with_mayor(), "remove the librarian now");
    settle(&mut app, &with_mayor());
    assert!(hamlet.residents().is_empty());
    session::shutdown(app.world_mut());
}

#[test]
fn a_hamlet_in_manual_mode_asks_before_each_edit_and_command() {
    let rt = Runtime::new().unwrap();
    let project = Scratch::new("oversight-manual-project");
    let (mut app, home) = app(
        "manual",
        &rt,
        vec![
            call(
                "edit",
                json!({"path": "a.txt", "old": "", "new": "one"}),
            ),
            call("read", json!({"path": "a.txt"})),
            bash("echo two > b.txt"),
            call(
                "edit",
                json!({"path": "a.txt", "old": "one", "new": "1"}),
            ),
            bash("echo three > c.txt"),
            says("done"),
        ],
    );
    let hamlet = Hamlet::new(home.root.clone());
    hamlet.work_on(Some(&project), "alice").unwrap();
    say(&mut app, &with_mayor(), "go");

    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].kind, Kind::Approval(Danger::Edit));
    assert_eq!(needs[0].about, "a.txt\nA new file:\none");
    assert_eq!(
        needs[0].answers,
        vec![Choice::Allow, Choice::Always, Choice::Deny]
    );
    assert!(!project.join("a.txt").exists());
    // Edits are allowed for the session, commands are not.
    answer(&mut app, needs[0].entity, Choice::Always, "");

    // The read between them did not wait.
    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].kind, Kind::Approval(Danger::Command));
    assert!(
        told(&mayor_log(&home)).contains(&"1\tone\n".to_string())
    );
    answer(&mut app, needs[0].entity, Choice::Allow, "");

    // The second edit ran unasked, and the next command asks again.
    let needs = waiting(&mut app, 1);
    assert_eq!(needs[0].about, "echo three > c.txt");
    assert_eq!(
        std::fs::read_to_string(project.join("a.txt")).unwrap(),
        "1"
    );
    assert!(project.join("b.txt").exists());
    answer(&mut app, needs[0].entity, Choice::Deny, "enough");
    settle(&mut app, &with_mayor());
    assert!(!project.join("c.txt").exists());
    // Nothing of the agent's own went into the project.
    assert!(!project.join("NOTES.md").exists());

    // In auto mode nothing waits.
    hamlet
        .set_mode(hamlet_rules::work::Mode::Auto, "alice")
        .unwrap();
    assert!(!hamlet.work().manual());
    session::shutdown(app.world_mut());
}

#[derive(Resource, Default)]
struct Heard(Vec<(Entity, Choice)>);

#[test]
fn a_plugin_petition_is_answered_like_any_and_the_answer_goes_back() {
    let rt = Runtime::new().unwrap();
    let (mut app, _home) = app("plugin", &rt, Vec::new());
    app.init_resource::<Heard>().add_observer(
        |settled: On<Settled>, mut heard: ResMut<Heard>| {
            heard.0.push((settled.entity, settled.choice));
        },
    );
    let asked = petition(
        app.world_mut(),
        "Roster",
        "Install Roster 0.1.0?".into(),
        vec![Choice::Allow, Choice::Deny],
    );
    let needs = queue(&mut app);
    assert_eq!(needs.len(), 1);
    assert_eq!(needs[0].kind, Kind::Plugin);

    // A choice the petition does not offer is not taken.
    answer(&mut app, asked, Choice::AskNextTime, "");
    assert_eq!(waits(app.world_mut()).len(), 1);

    answer(&mut app, asked, Choice::Allow, "");
    assert!(waits(app.world_mut()).is_empty());
    assert_eq!(
        app.world().resource::<Heard>().0,
        [(asked, Choice::Allow)]
    );
    session::shutdown(app.world_mut());
}
