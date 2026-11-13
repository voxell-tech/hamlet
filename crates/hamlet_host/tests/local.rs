//! The local host: start by key, stop, the folder lock, alarms armed
//! at start, and the commit hook.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use hamlet_actor_core::{
    ActorConfig, ActorEvent, ActorFactory, ActorStart, ShutdownKind,
};
use hamlet_actor_error::ActorError;
use hamlet_host::Host;
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};

#[path = "../examples/demo/counter.rs"]
mod counter;

fn factories() -> HashMap<String, ActorFactory> {
    counter::factories(counter::Sleep::WhenIdle)
}

async fn call(host: &Host, actor_id: &str, action: &str) -> i64 {
    call_with(host, actor_id, action, Vec::new()).await
}

async fn call_with(
    host: &Host,
    actor_id: &str,
    action: &str,
    args: Vec<u8>,
) -> i64 {
    let reply = host.action(actor_id, action, args).await.unwrap();
    counter::count(reply).unwrap()
}

/// Opens the actor's file as another process would, without waiting
/// for a lock.
fn probe(dir: &Path, kind: &str, key: &str) -> rusqlite::Result<i64> {
    let path =
        dir.join("actors").join(kind).join(format!("{key}.sqlite"));
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    conn.busy_timeout(Duration::ZERO)?;
    conn.query_row(
        "SELECT generation FROM _hamlet_actor",
        [],
        |row| row.get(0),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn starts_an_actor_by_its_kind_and_key() {
    let dir = Scratch::new("host-start");
    let host = Host::open(&dir, factories()).unwrap();

    let id = host.get_or_start("counter", "a").await.unwrap();
    assert_eq!(id, "counter/a");
    assert_eq!(call(&host, &id, "increment").await, 1);
    assert_eq!(host.get_or_start("counter", "a").await.unwrap(), id);
    assert_eq!(call(&host, &id, "increment").await, 2);

    let other = host.get_or_start("counter", "b").await.unwrap();
    assert_eq!(call(&host, &other, "get").await, 0);
    assert!(dir.join("actors/counter/a.sqlite").is_file());

    // The running actor holds its file in exclusive locking mode.
    assert!(probe(&dir, "counter", "a").is_err());

    for bad in ["", "../a", ".hidden", "a/b"] {
        assert!(host.get_or_start("counter", bad).await.is_err());
    }
    assert!(host.get_or_start("nobody", "a").await.is_err());
    host.stop_all().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_actor_closes_its_file_and_starts_again() {
    let dir = Scratch::new("host-stop");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    call(&host, &id, "increment").await;
    call(&host, &id, "increment").await;

    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    assert!(!host.is_running(&id).await);
    assert!(host.action(&id, "get", Vec::new()).await.is_err());
    assert_eq!(probe(&dir, "counter", "a").unwrap(), 1);

    host.get_or_start("counter", "a").await.unwrap();
    assert_eq!(call(&host, &id, "get").await, 2);
    host.stop_all().await.unwrap();
    assert_eq!(probe(&dir, "counter", "a").unwrap(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_host_is_refused_by_the_folder_lock() {
    let dir = Scratch::new("host-lock");
    let first = Host::open(&dir, factories()).unwrap();
    let id = first.get_or_start("counter", "a").await.unwrap();

    let Err(error) = Host::open(&dir, factories()) else {
        panic!("a second host opened the folder");
    };
    let message = error.to_string();
    assert!(message.contains("host.lock"), "{message}");
    assert_eq!(call(&first, &id, "increment").await, 1);

    first.stop_all().await.unwrap();
    drop(first);
    let again = Host::open(&dir, factories()).unwrap();
    let id = again.get_or_start("counter", "a").await.unwrap();
    assert_eq!(call(&again, &id, "get").await, 1);
    again.stop_all().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn boot_arms_a_stored_alarm_and_starts_the_actor_when_due() {
    let dir = Scratch::new("host-alarm");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    call_with(&host, &id, "schedule", 3000u64.to_le_bytes().to_vec())
        .await;
    host.stop_all().await.unwrap();
    drop(host);

    let host = Host::open(&dir, factories()).unwrap();
    assert!(host.boot().await.unwrap().is_empty());
    assert!(!host.is_running(&id).await);
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(10);
    while !host.is_running(&id).await
        || call(&host, &id, "get").await != 1
    {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    host.stop_all().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn boot_starts_an_actor_whose_alarm_came_due_while_down() {
    let dir = Scratch::new("host-due");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    call_with(&host, &id, "schedule", 200u64.to_le_bytes().to_vec())
        .await;
    host.stop_all().await.unwrap();
    drop(host);
    tokio::time::sleep(Duration::from_millis(400)).await;

    let host = Host::open(&dir, factories()).unwrap();
    assert_eq!(host.boot().await.unwrap(), vec![id.clone()]);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(call(&host, &id, "get").await, 1);
    host.stop_all().await.unwrap();

    // The alarm fired once and is gone, so the next boot starts
    // nothing.
    drop(host);
    let host = Host::open(&dir, factories()).unwrap();
    assert!(host.boot().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_committed_transaction_tells_the_host_and_a_rollback_does_not()
 {
    let dir = Scratch::new("host-commit");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    let mut commits = host.commits();
    while commits.try_recv().is_ok() {}

    assert_eq!(call(&host, &id, "discard").await, 0);
    assert!(commits.try_recv().is_err());
    assert_eq!(call(&host, &id, "increment").await, 1);
    assert_eq!(commits.try_recv().unwrap(), id);
    assert!(commits.try_recv().is_err());
    host.stop_all().await.unwrap();
}

/// The grace period an actor gets to finish its work is 15 seconds.
/// One with no work left stops well inside it.
#[tokio::test(flavor = "multi_thread")]
async fn an_actor_that_asks_to_sleep_or_be_destroyed_stops_at_once() {
    let dir = Scratch::new("host-ask");
    let host = Host::open(&dir, factories()).unwrap();
    for n in 0..10 {
        for action in ["sleep", "destroy"] {
            let key = format!("{action}-{n}");
            let id =
                host.get_or_start("counter", &key).await.unwrap();
            host.action(&id, action, Vec::new()).await.unwrap();
            // A stop holds the running list until the file is closed,
            // or removed for a destroy.
            let stopped =
                tokio::time::timeout(Duration::from_secs(2), async {
                    while host.is_running(&id).await {
                        tokio::time::sleep(Duration::from_millis(5))
                            .await;
                    }
                })
                .await;
            assert!(
                stopped.is_ok(),
                "{id} did not stop within 2s of asking to {action}"
            );
        }
        assert_eq!(
            probe(&dir, "counter", &format!("sleep-{n}")).unwrap(),
            1
        );
        assert!(
            !dir.join(format!("actors/counter/destroy-{n}.sqlite"))
                .exists()
        );
    }
}

/// An action the actor never answers fails with the structured
/// timeout error once the kind's action timeout passes.
#[tokio::test(flavor = "multi_thread")]
async fn an_action_with_no_reply_times_out() {
    let dir = Scratch::new("host-action-timeout");
    let config = ActorConfig {
        name: Some("mute".to_owned()),
        action_timeout: Duration::from_millis(200),
        ..ActorConfig::default()
    };
    let factories = HashMap::from([(
        "mute".to_owned(),
        ActorFactory::new(config, |start: ActorStart| {
            Box::pin(async move {
                let mut events = start.events;
                let mut unanswered = Vec::new();
                while let Some(event) = events.recv().await {
                    match event {
                        ActorEvent::Action { reply, .. } => {
                            unanswered.push(reply);
                        }
                        ActorEvent::SerializeState {
                            reply, ..
                        } => {
                            reply.send(Ok(Vec::new()));
                        }
                        ActorEvent::RunGracefulCleanup {
                            reply,
                            ..
                        }
                        | ActorEvent::ConnectionPreflight {
                            reply,
                            ..
                        }
                        | ActorEvent::ConnectionOpen {
                            reply, ..
                        }
                        | ActorEvent::DisconnectConn {
                            reply, ..
                        } => {
                            reply.send(Ok(()));
                        }
                        _ => {}
                    }
                }
                Ok(())
            })
        }),
    )]);
    let host = Host::open(&dir, factories).unwrap();
    let id = host.get_or_start("mute", "a").await.unwrap();

    let error = tokio::time::timeout(
        Duration::from_secs(5),
        host.action(&id, "wait", Vec::new()),
    )
    .await
    .expect("the action timeout did not end the call")
    .expect_err("an unanswered action must fail");
    let error = ActorError::extract(&error);
    assert_eq!(error.group(), "actor");
    assert_eq!(error.code(), "action_timed_out");
    assert_eq!(error.message(), "Action timed out");
    // A stop would wait out the grace period for the unanswered
    // action, so the test ends with the actor still running.
}

/// The host's caller is Hamlet's own code, so an action's error
/// reaches it with the actor's message and its causes.
#[tokio::test(flavor = "multi_thread")]
async fn an_action_error_keeps_the_actors_message() {
    let dir = Scratch::new("host-action-error");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();

    for (action, args, cause) in [
        ("nope", Vec::new(), "unknown action `nope`"),
        ("schedule", vec![1], "schedule takes a u64"),
    ] {
        let error = host.action(&id, action, args).await.unwrap_err();
        let text = format!("{error:#}");
        assert!(text.contains(cause), "{action}: {text}");
    }
    host.stop_all().await.unwrap();
}

/// Each new alarm replaces the actor's armed timer, so storing many
/// alarms leaves one sleeping task, not one per alarm.
#[tokio::test(flavor = "multi_thread")]
async fn a_new_alarm_replaces_the_armed_timer() {
    let dir = Scratch::new("host-one-timer");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    let schedule = |ms: u64| {
        call_with(&host, &id, "schedule", ms.to_le_bytes().to_vec())
    };
    let alive = || {
        tokio::runtime::Handle::current()
            .metrics()
            .num_alive_tasks()
    };

    // Each alarm is earlier than the last, so each is stored and
    // armed.
    // Read before the first alarm is armed, the count is at most one
    // lower, which only loosens the check.
    schedule(600_000).await;
    let before = alive();
    for n in 1..=20 {
        schedule(600_000 - n * 1000).await;
    }
    // The stores' own tasks end soon after; the timers do not.
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(5);
    let mut after = alive();
    while after > before + 2 && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
        after = alive();
    }
    assert!(
        after <= before + 2,
        "{before} tasks alive after one alarm, {after} after 21"
    );
    host.stop_all().await.unwrap();
}
