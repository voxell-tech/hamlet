//! A chief's line, which the realm keeps from its answer until its
//! agent takes it, and the stop of agents, one act of the realm.

use std::time::{Duration, Instant};

use hamlet_messages::address;
use hamlet_messages::agent::chat::{Stop, Stopped, Withdrawn};
use hamlet_messages::agent::{Input, InputId};
use hamlet_messages::realm::client::SayAnswer;
use hamlet_messages::realm::lines::{
    LineIs, ReadLine, StopAgents, Working,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{AgentNumber, RealmTask};
use hamlet_protocol::client::RequestId;

use super::stand_ins::{Callee, Took};
use super::{
    FOUR, MAYOR, Realm, THREE, error_code, founded, hamlet_with, say,
    submits_to, until,
};

async fn said(
    realm: &Realm,
    id: &RequestId,
    to: AgentNumber,
    text: &str,
) -> RealmTask {
    let said = realm.ask(&say(id, to, text)).await.unwrap();
    let SayAnswer::Accepted { task } = said else {
        panic!("the realm accepts the line: {said:?}");
    };
    task
}

async fn read(realm: &Realm, id: &RequestId) -> LineIs {
    realm.ask(&ReadLine(id.clone())).await.unwrap()
}

async fn stop(realm: &Realm, agents: &[AgentNumber]) -> u32 {
    let stopped = realm
        .ask(&StopAgents {
            agents: agents.to_vec(),
        })
        .await
        .unwrap();
    stopped.turns
}

fn took(realm: &Realm, agent: AgentNumber) -> Vec<Took> {
    realm
        .seen
        .took
        .lock()
        .unwrap()
        .iter()
        .filter(|(to, _)| *to == agent)
        .map(|(_, took)| took.clone())
        .collect()
}

fn stops(realm: &Realm, agent: AgentNumber) -> Vec<Stop> {
    took(realm, agent)
        .into_iter()
        .filter_map(|took| match took {
            Took::Stop(stop) => Some(stop),
            Took::Submit(_) => None,
        })
        .collect()
}

fn input(id: &RequestId) -> InputId {
    InputId(id.as_str().to_owned())
}

fn submitted(id: &RequestId) -> Took {
    Took::Submit(input(id))
}

fn withdrawn(id: &RequestId, text: &str) -> Withdrawn {
    Withdrawn {
        id: input(id),
        line: Input::Line {
            text: text.to_owned(),
            images: Vec::new(),
            files: Vec::new(),
            note: None,
            chief: Some("chief".to_owned()),
        },
    }
}

/// Whether `took` is a stop that names `lines` withdrawn.
fn stopped(took: &Took, lines: &[Withdrawn]) -> bool {
    matches!(took, Took::Stop(stop) if stop.withdrawn == lines)
}

/// The first line's delivery is held at the agent, the second waits
/// behind it, and the stop commits: the second is withdrawn, the
/// first is given, and the stop follows it and names the second.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_withdraws_the_lines_its_agent_has_not_taken() {
    let (realm, _) = hamlet_with(1).await;
    let (taken, kept) = (RequestId::new(), RequestId::new());
    realm.seen.hold_next(Callee::Submit, THREE);
    said(&realm, &taken, THREE, "dig the well").await;
    until("the first line waits at the agent", || {
        realm.seen.is_waiting(Callee::Submit, THREE)
    })
    .await;
    said(&realm, &kept, THREE, "then rest").await;

    let turns = stop(&realm, &[THREE]).await;
    let while_held =
        (read(&realm, &taken).await, read(&realm, &kept).await);
    realm.seen.release(Callee::Submit);
    until("the stop reaches the agent", || {
        !stops(&realm, THREE).is_empty()
    })
    .await;

    assert_eq!(turns, 1);
    assert_eq!(while_held, (LineIs::Waiting, LineIs::Withdrawn));
    assert_eq!(read(&realm, &taken).await, LineIs::Given);
    let took = took(&realm, THREE);
    assert_eq!(took.len(), 2, "{took:?}");
    assert_eq!(took[0], submitted(&taken));
    assert!(
        stopped(&took[1], &[withdrawn(&kept, "then rest")]),
        "{took:?}"
    );
    realm.host.shutdown().await.unwrap();
}

/// The line to 4 waits while 4 is down and the stop of 3 commits.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_of_one_agent_leaves_a_line_to_another() {
    let (realm, _) = hamlet_with(2).await;
    let line = RequestId::new();
    realm.seen.down(FOUR);
    said(&realm, &line, FOUR, "fetch water").await;

    let turns = stop(&realm, &[THREE]).await;
    let while_down = read(&realm, &line).await;
    realm.seen.up(FOUR);
    until("the line reaches 4", || {
        took(&realm, FOUR).contains(&submitted(&line))
    })
    .await;
    until("the stop reaches 3", || !stops(&realm, THREE).is_empty())
        .await;

    assert_eq!(turns, 0);
    assert_eq!(while_down, LineIs::Waiting);
    assert_eq!(read(&realm, &line).await, LineIs::Given);
    assert_eq!(took(&realm, FOUR), [submitted(&line)]);
    assert!(stopped(&took(&realm, THREE)[0], &[]));
    realm.host.shutdown().await.unwrap();
}

/// The stop is held at the agent while a line is said after it.
#[tokio::test(flavor = "multi_thread")]
async fn a_line_said_after_a_stop_is_given_after_it() {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.hold(Callee::Stop);
    stop(&realm, &[THREE]).await;
    until("the stop waits at the agent", || {
        realm.seen.is_waiting(Callee::Stop, THREE)
    })
    .await;
    let line = RequestId::new();

    said(&realm, &line, THREE, "then rest").await;
    let while_held = read(&realm, &line).await;
    realm.seen.release(Callee::Stop);
    until("the line reaches the agent", || {
        took(&realm, THREE).contains(&submitted(&line))
    })
    .await;

    assert_eq!(while_held, LineIs::Waiting);
    let took = took(&realm, THREE);
    assert_eq!(took.len(), 2, "{took:?}");
    assert!(stopped(&took[0], &[]), "{took:?}");
    assert_eq!(took[1], submitted(&line));
    realm.host.shutdown().await.unwrap();
}

/// At once is inside 500 ms, the time the realm waits for an agent
/// that answers late. A call to an agent that is down fails here in
/// a few milliseconds, over ten times less.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_with_the_agent_down_is_answered_at_once_and_reaches_it_when_it_returns()
 {
    let (realm, _) = hamlet_with(1).await;
    let (first, next) = (RequestId::new(), RequestId::new());
    realm.seen.down(THREE);
    said(&realm, &first, THREE, "dig the well").await;

    let begun = Instant::now();
    let turns = stop(&realm, &[THREE]).await;
    let answered = begun.elapsed();
    let while_down = read(&realm, &first).await;
    said(&realm, &next, THREE, "then rest").await;
    realm.seen.up(THREE);
    until("the line said after the stop reaches the agent", || {
        took(&realm, THREE).contains(&submitted(&next))
    })
    .await;

    assert!(
        answered < Duration::from_millis(500),
        "the stop was answered after {answered:?}"
    );
    assert_eq!(turns, 1);
    assert_eq!(while_down, LineIs::Withdrawn);
    let took = took(&realm, THREE);
    assert_eq!(took.len(), 2, "{took:?}");
    assert!(
        stopped(&took[0], &[withdrawn(&first, "dig the well")]),
        "{took:?}"
    );
    assert_eq!(took[1], submitted(&next));
    realm.host.shutdown().await.unwrap();
}

/// The mayor's hire waits at its held branch while a line is said
/// to it.
#[tokio::test(flavor = "multi_thread")]
async fn a_line_to_a_hire_being_hired_is_given_once_it_is_hired() {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    realm.seen.hold(Callee::Branch);
    founded(&realm).await;
    until("the mayor's drive waits on the held branch", || {
        realm.seen.is_waiting(Callee::Branch, MAYOR)
    })
    .await;
    let (line, next) = (RequestId::new(), RequestId::new());

    said(&realm, &line, MAYOR, "build the mill").await;
    let while_hiring =
        (read(&realm, &line).await, submits_to(&realm, MAYOR).len());
    realm.seen.release(Callee::Branch);
    said(&realm, &next, MAYOR, "then the barn").await;
    until("the next line reaches the mayor", || {
        took(&realm, MAYOR).contains(&submitted(&next))
    })
    .await;

    assert_eq!(while_hiring, (LineIs::Waiting, 0));
    assert_eq!(read(&realm, &line).await, LineIs::Given);
    assert_eq!(
        took(&realm, MAYOR),
        [submitted(&line), submitted(&next)]
    );
    realm.host.shutdown().await.unwrap();
}

/// A say repeated under its id gives its line once, and the same id
/// with other words is refused as reused.
#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_say_gives_its_line_once() {
    let (realm, _) = hamlet_with(0).await;
    let (id, next) = (RequestId::new(), RequestId::new());
    let line = say(&id, MAYOR, "build the mill");

    let first = realm.ask(&line).await.unwrap();
    let again = realm.ask(&line).await.unwrap();
    let other = realm.ask(&say(&id, MAYOR, "build the barn")).await;
    said(&realm, &next, MAYOR, "then the barn").await;
    until("the next line reaches the mayor", || {
        took(&realm, MAYOR).contains(&submitted(&next))
    })
    .await;
    let tasks = realm
        .stopped_rows(|db| {
            db.prepare(
                "SELECT id FROM task WHERE agent = 2 AND origin = \
                 'chief' ORDER BY id",
            )?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()
        })
        .await;

    let SayAnswer::Accepted { task } = &first else {
        panic!("the realm accepts the line: {first:?}");
    };
    assert_eq!(again, first);
    assert_eq!(error_code(other), "message.id_reused");
    assert_eq!(tasks.len(), 2);
    assert_eq!(task.0, tasks[0].to_string());
    assert_eq!(
        took(&realm, MAYOR),
        [submitted(&id), submitted(&next)]
    );
    realm.host.shutdown().await.unwrap();
}

/// The realm keeps its lines in memory, so once it starts again a
/// repeated say finds no line and gives it again; the agent takes a
/// repeated input once.
#[tokio::test(flavor = "multi_thread")]
async fn a_say_repeated_after_the_realm_starts_again_gives_its_line_again()
 {
    let (realm, _) = hamlet_with(0).await;
    let id = RequestId::new();
    let line = say(&id, MAYOR, "build the mill");
    let first = realm.ask(&line).await.unwrap();
    until("the line reaches the mayor", || {
        took(&realm, MAYOR).contains(&submitted(&id))
    })
    .await;
    let realm = realm.restart().await;

    let again = realm.ask(&line).await.unwrap();
    until("the line reaches the mayor again", || {
        took(&realm, MAYOR).len() == 2
    })
    .await;

    assert_eq!(again, first);
    assert_eq!(took(&realm, MAYOR), [submitted(&id), submitted(&id)]);
    realm.host.shutdown().await.unwrap();
}

/// 3 is down while 3 and 4 are stopped together; 4 answers its own
/// count.
#[tokio::test(flavor = "multi_thread")]
async fn one_unreachable_agent_does_not_fail_a_stop_of_all() {
    let (realm, _) = hamlet_with(2).await;
    realm.seen.down(THREE);
    realm.seen.turns.lock().unwrap().insert(FOUR, 2);

    let stopped = realm
        .ask(&StopAgents {
            agents: vec![THREE, FOUR],
        })
        .await;
    realm.seen.up(THREE);
    until("the stop reaches 3 once it is back", || {
        !stops(&realm, THREE).is_empty()
    })
    .await;

    assert_eq!(stopped.unwrap(), Stopped { turns: 2 });
    assert_eq!(stops(&realm, FOUR).len(), 1);
    realm.host.shutdown().await.unwrap();
}

/// The stand-in holds each stop as a frozen agent would. The realm
/// answers each 500 ms after its commit, the second's wait in the
/// lane behind the first included; the test allows a second, a
/// margin of 500 ms, over ten times what a commit and an answer take
/// on a busy machine.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_to_a_frozen_agent_is_answered_with_the_realms_count_within_a_second()
 {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.turns.lock().unwrap().insert(THREE, 5);
    realm.seen.hold(Callee::Stop);

    let begun = Instant::now();
    let idle = stop(&realm, &[THREE]).await;
    let idle_within = begun.elapsed();
    realm
        .call_as(&address::Agent(THREE), &Working { open: true })
        .await
        .unwrap();
    let begun = Instant::now();
    let busy = stop(&realm, &[THREE]).await;
    let busy_within = begun.elapsed();
    realm.seen.release(Callee::Stop);
    until("both held stops reach the agent", || {
        stops(&realm, THREE).len() == 2
    })
    .await;

    assert_eq!((idle, busy), (0, 1));
    assert!(idle_within < Duration::from_secs(1), "{idle_within:?}");
    assert!(busy_within < Duration::from_secs(1), "{busy_within:?}");
    realm.host.shutdown().await.unwrap();
}
