//! The local host: start by key, stop, the folder lock, alarms armed
//! at start, the commit hook, and how one actor's start or stop
//! leaves the others alone.

use std::collections::HashMap;
use std::future::Future;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use hamlet_actor_core::{
    ActorConfig, ActorContext, ActorEvent, ActorFactory, ActorStart,
    ColumnValue, ShutdownKind,
};
use hamlet_actor_error::ActorError;
use hamlet_host::Host;
use hamlet_testing::Scratch;
use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags};

#[path = "../examples/demo/counter.rs"]
mod counter;

fn factories() -> HashMap<String, ActorFactory> {
    counter::factories(counter::Sleep::WhenIdle)
}

/// How long an actor may take to answer while another actor stops.
/// An answer takes well under a millisecond.
const PROMPT: Duration = Duration::from_secs(1);

/// Awaits `work`, which must end within [`PROMPT`].
async fn prompt<T>(
    what: &str,
    work: impl Future<Output = anyhow::Result<T>>,
) -> T {
    tokio::time::timeout(PROMPT, work)
        .await
        .unwrap_or_else(|_| panic!("{what} took over {PROMPT:?}"))
        .unwrap()
}

/// The counters, and the kind `slow`, whose action `nap` takes
/// three seconds to answer. A stop of a `slow` actor waits for its
/// nap.
fn with_slow() -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("slow".to_owned()),
        ..ActorConfig::default()
    };
    let mut kinds = factories();
    kinds.insert(
        "slow".to_owned(),
        ActorFactory::new(config, |start: ActorStart| {
            Box::pin(async move {
                let mut events = start.events;
                while let Some(event) = events.recv().await {
                    match event {
                        ActorEvent::Action {
                            name, reply, ..
                        } => {
                            if name == "nap" {
                                tokio::time::sleep(
                                    Duration::from_secs(3),
                                )
                                .await;
                            }
                            reply.send(Ok(0i64
                                .to_le_bytes()
                                .to_vec()));
                        }
                        other => answer(other),
                    }
                }
                Ok(())
            })
        }),
    );
    kinds
}

/// Answers the lifecycle events a test actor has no use for.
fn answer(event: ActorEvent) {
    match event {
        ActorEvent::SerializeState { reply, .. } => {
            reply.send(Ok(Vec::new()));
        }
        ActorEvent::RunGracefulCleanup { reply, .. }
        | ActorEvent::ConnectionPreflight { reply, .. }
        | ActorEvent::ConnectionOpen { reply, .. }
        | ActorEvent::DisconnectConn { reply, .. } => {
            reply.send(Ok(()));
        }
        _ => {}
    }
}

/// Each generation's context, in the order they started.
type Contexts = Arc<Mutex<Vec<ActorContext>>>;

/// The kind `keeper`, which hands each generation's context to the
/// test, as a task the generation left behind would hold it. Its one
/// action replies with the number of rows in its table `marks`.
fn keeper(contexts: &Contexts) -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("keeper".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    let contexts = contexts.clone();
    HashMap::from([(
        "keeper".to_owned(),
        ActorFactory::new(config, move |start: ActorStart| {
            contexts.lock().push(start.ctx.clone());
            Box::pin(async move {
                let ActorStart {
                    ctx, mut events, ..
                } = start;
                ctx.sql()
                    .exec(
                        "CREATE TABLE IF NOT EXISTS marks (who TEXT)"
                            .to_owned(),
                    )
                    .await?;
                while let Some(event) = events.recv().await {
                    match event {
                        ActorEvent::Action { reply, .. } => {
                            let rows = ctx
                                .sql()
                                .execute(
                                    "SELECT COUNT(*) FROM marks"
                                        .to_owned(),
                                    None,
                                )
                                .await
                                .map(|result| result.rows);
                            reply.send(rows.map(|rows| {
                                match rows
                                    .first()
                                    .and_then(|row| row.first())
                                {
                                    Some(ColumnValue::Integer(n)) => {
                                        n.to_le_bytes().to_vec()
                                    }
                                    _ => Vec::new(),
                                }
                            }));
                        }
                        other => answer(other),
                    }
                }
                Ok(())
            })
        }),
    )])
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
    assert!(!host.is_running(&id));
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
    assert!(!host.is_running(&id));
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(10);
    while !host.is_running(&id) || call(&host, &id, "get").await != 1
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
            // An actor counts as running until its stop closed its
            // file, or removed it for a destroy.
            let stopped =
                tokio::time::timeout(Duration::from_secs(2), async {
                    while host.is_running(&id) {
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

/// The realm folder [`stuck_stop_child`] opens. Unset, the child
/// does nothing.
const STUCK_CHILD_HOME: &str = "HAMLET_HOST_TEST_STUCK_HOME";

/// One actor's stop stuck for good leaves the other actors' actions
/// and starts alone. The scenario runs in a child process, because
/// its fault point is set for the whole process.
#[test]
fn a_stuck_stop_does_not_stall_other_actors() {
    let home = Scratch::new("host-stuck-stop");
    let faults = home.join("faults");
    std::fs::create_dir_all(&faults).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "stuck_stop_child", "--nocapture"])
        .env(STUCK_CHILD_HOME, &*home)
        .env("HAMLET_FAULT", "tx-open")
        .env("HAMLET_FAULT_DIR", &faults)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The child of [`a_stuck_stop_does_not_stall_other_actors`]. Actor
/// `b`'s increment stops for good inside its transaction at the fault
/// point `tx-open`, and a stop of `b` waits for it.
#[test]
fn stuck_stop_child() {
    let Some(home) = std::env::var_os(STUCK_CHILD_HOME) else {
        return;
    };
    let faults =
        PathBuf::from(std::env::var_os("HAMLET_FAULT_DIR").unwrap());
    // Never dropped: a runtime's drop waits for its worker threads,
    // and one stays stopped at the fault point.
    let runtime = ManuallyDrop::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap(),
    );
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(30), async {
            let host =
                Host::open(Path::new(&home), factories()).unwrap();
            let a = host.get_or_start("counter", "a").await.unwrap();
            let b = host.get_or_start("counter", "b").await.unwrap();
            tokio::spawn({
                let host = host.clone();
                let b = b.clone();
                async move {
                    let _ = host
                        .action(&b, "increment", Vec::new())
                        .await;
                }
            });
            while !faults.join("host-paused").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            tokio::spawn({
                let host = host.clone();
                async move {
                    let _ = host.stop(&b, ShutdownKind::Sleep).await;
                }
            });
            tokio::time::sleep(Duration::from_millis(200)).await;
            prompt(
                "a get while b's stop is stuck",
                host.action(&a, "get", Vec::new()),
            )
            .await;
            prompt(
                "a start while b's stop is stuck",
                host.get_or_start("counter", "c"),
            )
            .await;
        })
        .await
        .expect("the child did not finish");
    });
}

/// A stop that waits for a slow action keeps only its own actor
/// waiting.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_stop_does_not_stall_other_actors() {
    let dir = Scratch::new("host-slow-stop");
    let host = Host::open(&dir, with_slow()).unwrap();
    let a = host.get_or_start("counter", "a").await.unwrap();
    let b = host.get_or_start("slow", "b").await.unwrap();
    let nap = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { host.action(&b, "nap", Vec::new()).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let stop = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { host.stop(&b, ShutdownKind::Sleep).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    prompt("a get while b stops", host.action(&a, "get", Vec::new()))
        .await;
    prompt(
        "a start while b stops",
        host.get_or_start("counter", "c"),
    )
    .await;

    nap.await.unwrap().unwrap();
    stop.await.unwrap().unwrap();
    host.stop_all().await.unwrap();
}

/// Starts of one key that race make one actor, with one generation.
#[tokio::test(flavor = "multi_thread")]
async fn two_starts_of_one_key_make_one_actor() {
    let dir = Scratch::new("host-two-starts");
    let host = Host::open(&dir, factories()).unwrap();
    let starts: Vec<_> = (0..8)
        .map(|_| {
            let host = host.clone();
            tokio::spawn(async move {
                host.get_or_start("counter", "a").await
            })
        })
        .collect();
    for start in starts {
        assert_eq!(start.await.unwrap().unwrap(), "counter/a");
    }
    assert_eq!(call(&host, "counter/a", "increment").await, 1);
    host.stop_all().await.unwrap();
    assert_eq!(probe(&dir, "counter", "a").unwrap(), 1);
}

/// A start of an actor that is stopping waits for that stop, and only
/// for it, then starts the actor's next generation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_start_during_its_actors_stop_waits_and_starts_it_fresh() {
    let dir = Scratch::new("host-start-in-stop");
    let host = Host::open(&dir, with_slow()).unwrap();
    let a = host.get_or_start("counter", "a").await.unwrap();
    let b = host.get_or_start("slow", "b").await.unwrap();
    let nap = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { host.action(&b, "nap", Vec::new()).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let stop = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { host.stop(&b, ShutdownKind::Sleep).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let start = tokio::spawn({
        let host = host.clone();
        async move { host.get_or_start("slow", "b").await }
    });

    prompt(
        "a get while b's start waits",
        host.action(&a, "get", Vec::new()),
    )
    .await;
    assert!(!stop.is_finished(), "b stopped before its nap ended");
    assert!(!start.is_finished(), "b started while it was stopping");

    nap.await.unwrap().unwrap();
    stop.await.unwrap().unwrap();
    assert_eq!(start.await.unwrap().unwrap(), b);
    host.action(&b, "get", Vec::new()).await.unwrap();
    host.stop_all().await.unwrap();
    assert_eq!(probe(&dir, "slow", "b").unwrap(), 2);
}

/// What a `keeper` replied: the rows in its table `marks`.
async fn marks(host: &Host, actor_id: &str) -> i64 {
    let reply =
        host.action(actor_id, "count", Vec::new()).await.unwrap();
    i64::from_le_bytes(reply.try_into().unwrap())
}

/// A task an ended generation left behind holds that generation's
/// context. Its statements must not reach the actor's next
/// generation.
#[tokio::test(flavor = "multi_thread")]
async fn an_ended_generation_cannot_write_to_the_next() {
    let dir = Scratch::new("host-ended-write");
    let contexts = Contexts::default();
    let host = Host::open(&dir, keeper(&contexts)).unwrap();
    let id = host.get_or_start("keeper", "t").await.unwrap();
    let ended = contexts.lock()[0].clone();
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    host.get_or_start("keeper", "t").await.unwrap();

    let in_transaction = async {
        let tx = ended.sql().begin_transaction(None).await?;
        tx.execute("INSERT INTO marks VALUES ('ended')", None)
            .await?;
        tx.commit().await
    }
    .await;
    assert!(
        in_transaction.is_err(),
        "the ended generation's transaction committed"
    );
    let alone = ended
        .sql()
        .execute(
            "INSERT INTO marks VALUES ('ended')".to_owned(),
            None,
        )
        .await;
    assert!(alone.is_err(), "the ended generation's insert ran");
    assert_eq!(marks(&host, &id).await, 0);
    host.stop_all().await.unwrap();
}

/// A transaction an ended generation left open expires after the next
/// generation started. Its rollback fails on the ended generation's
/// closed file, and the core reports that as a crash of the ended
/// generation, which must not stop the next one.
#[tokio::test(flavor = "multi_thread")]
async fn an_ended_generations_crash_does_not_stop_the_next() {
    let dir = Scratch::new("host-ended-crash");
    let contexts = Contexts::default();
    let host = Host::open(&dir, keeper(&contexts)).unwrap();
    let id = host.get_or_start("keeper", "t").await.unwrap();
    let ended = contexts.lock()[0].clone();
    let leaked = ended
        .sql()
        .begin_transaction(Some(Duration::from_millis(500)))
        .await
        .unwrap();
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    host.get_or_start("keeper", "t").await.unwrap();

    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert!(leaked.rollback().await.is_err(), "it never expired");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(host.is_running(&id));
    assert_eq!(marks(&host, &id).await, 0);
    host.stop_all().await.unwrap();
}
