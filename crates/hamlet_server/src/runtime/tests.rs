use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use hamlet_testing::Scratch;
use rusqlite::trace::{TraceEvent, TraceEventCodes};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

use super::{Serving, Since};
use crate::harness::{Config, Harnesses};
use crate::protocol::Protocol;
use crate::record::{
    Actor, Admit, Change, FoundRecord, RecordOperation,
};
use crate::service::Service;
use crate::{Home, Operation, OperationId, Overrides, Store};

thread_local! {
    // The trace hook is a plain function with no data of its own, so
    // it counts on the thread that runs the statement: the test's.
    static STATEMENTS: Cell<u64> = const { Cell::new(0) };
}

fn count(event: TraceEvent<'_>) {
    if let TraceEvent::Stmt(..) = event {
        STATEMENTS.set(STATEMENTS.get() + 1);
    }
}

fn run(store: &mut Store, operation: &impl Operation) {
    let id = OperationId {
        runtime: crate::RuntimeInstance::new("seed").unwrap(),
        key: uuid::Uuid::new_v4().to_string(),
    };
    store.run(&id, operation).unwrap();
}

fn change(change: Change) -> RecordOperation {
    RecordOperation {
        hamlet: "home".into(),
        actor: Actor::Chief("chief".into()),
        at: 1,
        reason: "Seeded".into(),
        task: None,
        change,
    }
}

/// The name of agent `index`, in the letters a name may use.
fn name(index: usize) -> String {
    let mut name = String::from("agent-");
    let mut rest = index;
    loop {
        name.push(char::from(b'a' + (rest % 26) as u8));
        rest /= 26;
        if rest == 0 {
            return name;
        }
    }
}

/// A serving home of `agents` agents besides its mayor, with no
/// runtime and no client.
fn serving(tag: &str, agents: usize) -> (Scratch, Serving) {
    let scratch =
        Scratch::new(&format!("{tag}-{}", uuid::Uuid::new_v4()));
    let mut home = Home::open(&scratch).unwrap();
    let store = home.store();
    run(
        store,
        &FoundRecord {
            hamlet: "home".into(),
            name: "founder".into(),
            purpose: "Leads".into(),
            chief: "chief".into(),
            at: 0,
        },
    );
    let mut limits = store.record("home").unwrap().unwrap().limits;
    limits.chief.most_agents = i64::try_from(agents).unwrap();
    run(
        store,
        &change(Change::SetLimits {
            limits,
            runs: Vec::new(),
        }),
    );
    for index in 0..agents {
        run(
            store,
            &change(Change::Admit(Admit {
                name: name(index),
                purpose: "Works".into(),
                model: None,
                character: String::new(),
                roads: Vec::new(),
            })),
        );
    }
    let mut service =
        Service::new(home, Overrides::none(true)).unwrap();
    crate::chats::load(&mut service).unwrap();
    let layout = service.home.layout().clone();
    let (harnesses, notices) = Harnesses::start(
        &layout,
        service.home.store(),
        Config::unused(),
    )
    .unwrap();
    assert!(notices.is_empty());
    let state = service.state(0, 0).unwrap();
    assert_eq!(state.agents.len(), agents + 1);
    let serving = Serving {
        service,
        harnesses,
        protocol: Protocol::new().unwrap(),
        clients: HashMap::new(),
        state,
        next: 0,
        events: mpsc::channel(1).0,
        closing: watch::channel(false).1,
        tasks: JoinSet::new(),
        reads: JoinSet::new(),
        reading: HashMap::new(),
        writes: JoinSet::new(),
        ready: VecDeque::new(),
    };
    (scratch, serving)
}

/// Counts the statements of the store of `serving` from now on.
fn trace(serving: &mut Serving) {
    serving
        .service
        .db()
        .trace_v2(TraceEventCodes::SQLITE_TRACE_STMT, Some(count));
    STATEMENTS.set(0);
}

#[tokio::test]
async fn an_idle_tick_runs_no_statement() {
    let (_scratch, mut serving) = serving("idle-tick", 3);
    let mut since = Since::new();
    serving.on_tick(&mut since).await.unwrap();
    trace(&mut serving);
    for _ in 0..20 {
        serving.on_tick(&mut since).await.unwrap();
    }
    assert_eq!(STATEMENTS.get(), 0);
}

#[tokio::test]
async fn a_skill_put_in_a_house_by_hand_shows_on_the_next_tick() {
    let (scratch, mut serving) = serving("hand-skill", 1);
    let mut since = Since::new();
    serving.on_tick(&mut since).await.unwrap();
    let skill = scratch.join("houses/home/1/skills/maps");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: maps\ndescription: Read maps.\n---\n",
    )
    .unwrap();
    let skills = |serving: &Serving| {
        serving
            .state
            .agents
            .iter()
            .find(|a| a.ref_.number == 1)
            .unwrap()
            .skills
            .clone()
    };
    serving.on_tick(&mut since).await.unwrap();
    assert_eq!(skills(&serving), ["maps"]);
}

/// Times idle ticks and rescans of the tools folder in a hamlet of a
/// hundred agents. Run it with `cargo test -p hamlet_server --lib
/// idle_ticks -- --ignored --nocapture`. On the owner's Mac on
/// 2026-10-23, in the dev profile: 1.27 ms and no statement an idle
/// tick, nearly all of it reading the 101 houses' skills; 26 µs a
/// rescan.
#[tokio::test]
#[ignore = "a measurement, not a check"]
#[expect(
    clippy::print_stdout,
    reason = "the measurement is read from its output"
)]
async fn idle_ticks_of_a_hundred_agents() {
    let (_scratch, mut serving) = serving("hundred-ticks", 100);
    let mut since = Since::new();
    serving.on_tick(&mut since).await.unwrap();
    trace(&mut serving);
    let started = Instant::now();
    for _ in 0..200 {
        serving.on_tick(&mut since).await.unwrap();
    }
    let each = started.elapsed() / 200;
    println!(
        "an idle tick: {each:?}, {} statements",
        STATEMENTS.get() / 200
    );
    let started = Instant::now();
    for _ in 0..20 {
        serving.harnesses.refresh_tools().await;
    }
    println!("a rescan: {:?}", started.elapsed() / 20);
}
