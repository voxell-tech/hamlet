//! The local host: start by key, calls and their deadlines, stop,
//! shutdown, the folder lock, the deadline a stop leaves, boot,
//! destroy, how one actor's start or stop leaves the others alone,
//! and the deadline of a start.

use std::collections::HashMap;
use std::future::Future;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use act::{DEADLINE, act};
use hamlet_actor_core::{
    ActorConfig, ActorContext, ActorEvent, ActorFactory, ActorStart,
    ColumnValue, ShutdownKind,
};
use hamlet_actor_error::ActorError;
use hamlet_host::{Address, Cause, Host, HostEvent, HostOptions};
use hamlet_testing::Scratch;
use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags};

#[path = "../examples/demo/act.rs"]
mod act;
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

/// The counters, and two [`napper`] kinds: `slow`, with no database,
/// and `brittle`, with one.
fn with_nappers() -> HashMap<String, ActorFactory> {
    let mut kinds = factories();
    kinds.insert("slow".to_owned(), napper("slow", false));
    kinds.insert("brittle".to_owned(), napper("brittle", true));
    kinds
}

/// A kind whose action `nap` takes three seconds to answer, so a stop
/// of it waits for the nap. With a database, its action `unmark`
/// drops the host's row from its file, so its stop cannot mark it
/// stopped. Every action answers a count of 0.
fn napper(kind: &str, has_database: bool) -> ActorFactory {
    let config = ActorConfig {
        name: Some(kind.to_owned()),
        has_database,
        ..ActorConfig::default()
    };
    ActorFactory::new(config, |start: ActorStart| {
        Box::pin(async move {
            let ActorStart {
                ctx, mut events, ..
            } = start;
            while let Some(event) = events.recv().await {
                match event {
                    ActorEvent::Action { name, reply, .. } => {
                        let done = match name.as_str() {
                            "nap" => {
                                tokio::time::sleep(
                                    Duration::from_secs(3),
                                )
                                .await;
                                Ok(())
                            }
                            "unmark" => ctx
                                .sql()
                                .exec("DROP TABLE _hamlet_actor")
                                .await
                                .map(drop),
                            _ => Ok(()),
                        };
                        reply.send(
                            done.map(|()| {
                                0i64.to_le_bytes().to_vec()
                            }),
                        );
                    }
                    other => counter::answer(other),
                }
            }
            Ok(())
        })
    })
}

/// Each generation's context, in the order they started.
type Contexts = Arc<Mutex<Vec<ActorContext>>>;

/// The kind `keeper`, which hands each generation's context to the
/// test, as a task the generation left behind would hold it. Its one
/// action replies with the number of rows in its table `marks`, as a
/// counter replies with its count. Its start ends once the table is
/// made, so the context is there when the start returns: the core
/// calls a factory on a task of its own.
fn keeper(contexts: &Contexts) -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("keeper".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    let contexts = contexts.clone();
    HashMap::from([(
        "keeper".to_owned(),
        ActorFactory::new_with_manual_startup_ready(
            config,
            move |start: ActorStart| {
                contexts.lock().push(start.ctx.clone());
                Box::pin(async move {
                    let ActorStart {
                        ctx,
                        mut events,
                        startup_ready,
                        ..
                    } = start;
                    ctx.sql()
                        .exec(
                            "CREATE TABLE IF NOT EXISTS marks (who \
                             TEXT)"
                                .to_owned(),
                        )
                        .await?;
                    if let Some(ready) = startup_ready {
                        let _ = ready.send(Ok(()));
                    }
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
                                        Some(
                                            ColumnValue::Integer(n),
                                        ) => n.to_le_bytes().to_vec(),
                                        _ => Vec::new(),
                                    }
                                }));
                            }
                            other => counter::answer(other),
                        }
                    }
                    Ok(())
                })
            },
        ),
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
    let reply = act(host, actor_id, action, args).await.unwrap();
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
    host.shutdown().await.unwrap();
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
    assert_eq!(probe(&dir, "counter", "a").unwrap(), 1);

    // A call starts the stopped actor again.
    assert_eq!(call(&host, &id, "get").await, 2);
    host.shutdown().await.unwrap();
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

    first.shutdown().await.unwrap();
    drop(first);
    let again = Host::open(&dir, factories()).unwrap();
    let id = again.get_or_start("counter", "a").await.unwrap();
    assert_eq!(call(&again, &id, "get").await, 1);
    again.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn boot_arms_a_stored_alarm_and_starts_the_actor_when_due() {
    let dir = Scratch::new("host-alarm");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    call_with(&host, &id, "schedule", 3000u64.to_le_bytes().to_vec())
        .await;
    host.shutdown().await.unwrap();
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
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn boot_starts_an_actor_whose_alarm_came_due_while_down() {
    let dir = Scratch::new("host-due");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    call_with(&host, &id, "schedule", 200u64.to_le_bytes().to_vec())
        .await;
    host.shutdown().await.unwrap();
    drop(host);
    tokio::time::sleep(Duration::from_millis(400)).await;

    let host = Host::open(&dir, factories()).unwrap();
    assert_eq!(host.boot().await.unwrap(), vec![id.clone()]);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(call(&host, &id, "get").await, 1);
    host.shutdown().await.unwrap();

    // The alarm fired once and is gone, so the next boot starts
    // nothing.
    drop(host);
    let host = Host::open(&dir, factories()).unwrap();
    assert!(host.boot().await.unwrap().is_empty());
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
            act(&host, &id, action, Vec::new()).await.unwrap();
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
/// timeout error once the kind's action timeout passes, though the
/// call's own deadline is far longer.
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
                        other => counter::answer(other),
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
        act(&host, &id, "wait", Vec::new()),
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
        let error = act(&host, &id, action, args).await.unwrap_err();
        let text = format!("{error:#}");
        assert!(text.contains(cause), "{action}: {text}");
    }
    host.shutdown().await.unwrap();
}

/// A running actor's alarms live in its core, and the host keeps no
/// timer or task for them, so many alarms leave no task behind.
#[tokio::test(flavor = "multi_thread")]
async fn many_alarms_of_a_running_actor_leave_no_task_behind() {
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

    // Each alarm is earlier than the last, so the core pushes each.
    schedule(600_000).await;
    let before = alive();
    for n in 1..=20 {
        schedule(600_000 - n * 1000).await;
    }
    // Tasks of the actions themselves end soon after.
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
    host.shutdown().await.unwrap();
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
                    let _ =
                        act(&host, &b, "increment", Vec::new()).await;
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
                act(&host, &a, "get", Vec::new()),
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
    let host = Host::open(&dir, with_nappers()).unwrap();
    let a = host.get_or_start("counter", "a").await.unwrap();
    let b = host.get_or_start("slow", "b").await.unwrap();
    let nap = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { act(&host, &b, "nap", Vec::new()).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let stop = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { host.stop(&b, ShutdownKind::Sleep).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    prompt("a get while b stops", act(&host, &a, "get", Vec::new()))
        .await;
    prompt(
        "a start while b stops",
        host.get_or_start("counter", "c"),
    )
    .await;

    nap.await.unwrap().unwrap();
    stop.await.unwrap().unwrap();
    host.shutdown().await.unwrap();
}

/// Where a [`blocker`]'s event loop blocks: it waits on the receiver
/// until the test drops the sender.
type Gate = Arc<Mutex<std::sync::mpsc::Receiver<()>>>;

/// A statement that runs for seconds: about 7 on a machine where
/// SQLite counts 14 million rows a second.
const LONG_STATEMENT: &str = "WITH RECURSIVE c(x) AS (SELECT 1 \
                              UNION ALL SELECT x + 1 FROM c WHERE x \
                              < 100000000) SELECT COUNT(*) FROM c";

/// The kind `blocker`, whose event loop blocks in synchronous code at
/// its action `block`, until `gate` closes, as an actor stuck in a
/// blocking call does. Its action `scan` runs [`LONG_STATEMENT`] from
/// the event loop. Each sends on `blocked` first. Its grace at a stop
/// is 200 ms, so its core soon gives the event loop up and waits for
/// it to end. Every action answers a count of 0.
fn blocker(
    gate: &Gate,
    blocked: tokio::sync::mpsc::UnboundedSender<()>,
) -> ActorFactory {
    let config = ActorConfig {
        name: Some("blocker".to_owned()),
        has_database: true,
        sleep_grace_period: Duration::from_millis(200),
        ..ActorConfig::default()
    };
    let gate = gate.clone();
    ActorFactory::new(config, move |start: ActorStart| {
        let gate = gate.clone();
        let blocked = blocked.clone();
        Box::pin(async move {
            let ActorStart {
                ctx, mut events, ..
            } = start;
            while let Some(event) = events.recv().await {
                match event {
                    ActorEvent::Action { name, reply, .. } => {
                        match name.as_str() {
                            "block" => {
                                let _ = blocked.send(());
                                let _ = gate.lock().recv();
                            }
                            "scan" => {
                                let _ = blocked.send(());
                                let _ = ctx
                                    .sql()
                                    .exec(LONG_STATEMENT.to_owned())
                                    .await;
                            }
                            _ => {}
                        }
                        reply.send(Ok(0i64.to_le_bytes().to_vec()));
                    }
                    other => counter::answer(other),
                }
            }
            Ok(())
        })
    })
}

/// The stop deadline the tests of a blocked actor give their host.
const STOP_DEADLINE: Duration = Duration::from_secs(2);

/// The counters and a [`blocker`] that waits on `gate`, run by a host
/// with [`STOP_DEADLINE`].
fn blocked_host(
    dir: &Path,
    gate: &Gate,
    blocked: &tokio::sync::mpsc::UnboundedSender<()>,
) -> Host {
    let mut kinds = factories();
    kinds
        .insert("blocker".to_owned(), blocker(gate, blocked.clone()));
    let options = HostOptions {
        stop_deadline: STOP_DEADLINE,
        ..HostOptions::default()
    };
    Host::open_with(dir, kinds, options).unwrap()
}

/// Calls the action `action`, `block` or `scan`, of the [`blocker`]
/// `actor_id`, and returns once its event loop blocks.
async fn block(
    host: &Host,
    actor_id: &str,
    action: &'static str,
    blocks: &mut tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    tokio::spawn({
        let host = host.clone();
        let actor_id = actor_id.to_owned();
        async move { act(&host, &actor_id, action, Vec::new()).await }
    });
    blocks.recv().await.unwrap();
}

/// Whether `took`, the time a stop of a blocked actor took, is the
/// stop deadline and at most the grace of its file's close past it.
fn at_the_deadline(took: Duration) -> bool {
    took >= STOP_DEADLINE
        && took < STOP_DEADLINE + Duration::from_secs(2)
}

/// A stop has a deadline. A shutdown with an actor whose event loop
/// blocks in synchronous code returns at the deadline, and names that
/// actor, and only it, as not stopped. The other actors stop cleanly.
/// The folder and the actor's file are free once the shutdown has
/// returned, while the actor's code still blocks: a new host opens
/// the folder, and its boot starts the actor again from its file,
/// which kept its running mark.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shutdown_returns_at_the_stop_deadline_of_a_blocked_actor()
{
    let dir = Scratch::new("host-stop-blocked");
    // Dropped last, also when an assertion fails: the runtime's drop
    // waits for the blocked thread.
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate: Gate = Arc::new(Mutex::new(gate));
    let (blocked, mut blocks) =
        tokio::sync::mpsc::unbounded_channel();
    let host = blocked_host(&dir, &gate, &blocked);
    let counter = host.get_or_start("counter", "a").await.unwrap();
    call(&host, &counter, "increment").await;
    let stuck = host.get_or_start("blocker", "b").await.unwrap();
    block(&host, &stuck, "block", &mut blocks).await;

    let begun = std::time::Instant::now();
    let shutdown = tokio::time::timeout(
        Duration::from_secs(20),
        host.shutdown(),
    )
    .await;
    let took = begun.elapsed();
    let error = format!(
        "{:#}",
        shutdown
            .unwrap_or_else(|_| panic!(
                "the shutdown waited {took:?}"
            ))
            .unwrap_err()
    );
    assert!(at_the_deadline(took), "{took:?}");
    assert!(error.contains(&stuck), "{error}");
    assert!(!error.contains(&counter), "{error}");

    let host = blocked_host(&dir, &gate, &blocked);
    assert_eq!(
        host.boot().await.unwrap(),
        std::slice::from_ref(&stuck)
    );
    assert_eq!(call(&host, &stuck, "get").await, 0);
    assert_eq!(call(&host, &counter, "get").await, 1);
    host.shutdown().await.unwrap();
    drop(release);
}

/// A stop has its deadline also while a statement of the actor still
/// runs on its file's thread, and keeps the stop from closing the
/// file. The shutdown returns once the deadline and the close's grace
/// have passed, and names the actor. The file stays open until the
/// statement ends, and keeps its running mark: the next boot starts
/// the actor again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shutdown_returns_while_a_statement_holds_the_file() {
    let dir = Scratch::new("host-stop-statement");
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate: Gate = Arc::new(Mutex::new(gate));
    let (blocked, mut blocks) =
        tokio::sync::mpsc::unbounded_channel();
    let host = blocked_host(&dir, &gate, &blocked);
    let stuck = host.get_or_start("blocker", "b").await.unwrap();
    block(&host, &stuck, "scan", &mut blocks).await;

    let begun = std::time::Instant::now();
    let error = format!("{:#}", host.shutdown().await.unwrap_err());
    let took = begun.elapsed();
    assert!(at_the_deadline(took), "{took:?}");
    assert!(error.contains(&stuck), "{error}");
    assert!(
        probe(&dir, "blocker", "b").is_err(),
        "the statement ended before the shutdown returned"
    );

    let freed = std::time::Instant::now();
    while probe(&dir, "blocker", "b").is_err() {
        assert!(freed.elapsed() < Duration::from_secs(120));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let host = blocked_host(&dir, &gate, &blocked);
    assert_eq!(
        host.boot().await.unwrap(),
        std::slice::from_ref(&stuck)
    );
    assert_eq!(call(&host, &stuck, "get").await, 0);
    host.shutdown().await.unwrap();
    drop(release);
}

/// A stop under way that misses its deadline fails each caller that
/// waited for it, a call and a second stop, with
/// `actor.shutdown_timeout`. From then on the actor is not running, a
/// call to it fails at once, and a shutdown names it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_past_its_deadline_fails_its_waiters_and_later_calls()
{
    let dir = Scratch::new("host-stop-waiters");
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate: Gate = Arc::new(Mutex::new(gate));
    let (blocked, mut blocks) =
        tokio::sync::mpsc::unbounded_channel();
    let host = blocked_host(&dir, &gate, &blocked);
    let stuck = host.get_or_start("blocker", "b").await.unwrap();
    block(&host, &stuck, "block", &mut blocks).await;

    let begun = std::time::Instant::now();
    let stop = |host: &Host| {
        let host = host.clone();
        let stuck = stuck.clone();
        tokio::spawn(async move {
            host.stop(&stuck, ShutdownKind::Sleep).await
        })
    };
    let first = stop(&host);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let waiting_call = tokio::spawn({
        let host = host.clone();
        let stuck = stuck.clone();
        async move { act(&host, &stuck, "get", Vec::new()).await }
    });
    let second = stop(&host);
    for (who, ended) in [
        ("the stop", first.await.unwrap()),
        ("the waiting call", waiting_call.await.unwrap().map(drop)),
        ("the second stop", second.await.unwrap()),
    ] {
        let error = ended.expect_err(who);
        assert!(
            is_actor_error(&error, "shutdown_timeout"),
            "{error:#}"
        );
    }
    let took = begun.elapsed();
    assert!(at_the_deadline(took), "{took:?}");

    assert!(!host.is_running(&stuck));
    let error = tokio::time::timeout(
        PROMPT,
        act(&host, &stuck, "get", Vec::new()),
    )
    .await
    .expect("a call to the stuck actor waited")
    .unwrap_err();
    assert!(is_actor_error(&error, "shutdown_timeout"), "{error:#}");
    let error = format!("{:#}", host.shutdown().await.unwrap_err());
    assert!(error.contains(&stuck), "{error}");
    drop(release);
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
    host.shutdown().await.unwrap();
    assert_eq!(probe(&dir, "counter", "a").unwrap(), 1);
}

/// A start of an actor that is stopping waits for that stop, and only
/// for it, then starts the actor's next generation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_start_during_its_actors_stop_waits_and_starts_it_fresh() {
    let dir = Scratch::new("host-start-in-stop");
    let host = Host::open(&dir, with_nappers()).unwrap();
    let a = host.get_or_start("counter", "a").await.unwrap();
    let b = host.get_or_start("slow", "b").await.unwrap();
    let nap = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { act(&host, &b, "nap", Vec::new()).await }
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
        act(&host, &a, "get", Vec::new()),
    )
    .await;
    assert!(!stop.is_finished(), "b stopped before its nap ended");
    assert!(!start.is_finished(), "b started while it was stopping");

    nap.await.unwrap().unwrap();
    stop.await.unwrap().unwrap();
    assert_eq!(start.await.unwrap().unwrap(), b);
    act(&host, &b, "get", Vec::new()).await.unwrap();
    host.shutdown().await.unwrap();
    assert_eq!(probe(&dir, "slow", "b").unwrap(), 2);
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
    assert_eq!(call(&host, &id, "count").await, 0);
    host.shutdown().await.unwrap();
}

/// A transaction an ended generation left open expires after the
/// actor's next generation started. Its rollback fails on the ended
/// generation's closed file, and the core reports that as a crash of
/// the ended generation, which must not stop the actor. If
/// `made_again`, the actor is destroyed and its key made again before
/// that: the new file counts its generations from the start again.
async fn an_ended_generations_crash_leaves_the_actor_running(
    made_again: bool,
) {
    /// How long the leaked transaction lives.
    const LEAK: Duration = Duration::from_millis(1500);
    let dir = Scratch::new("host-ended-crash");
    let contexts = Contexts::default();
    let host = Host::open(&dir, keeper(&contexts)).unwrap();
    let id = host.get_or_start("keeper", "t").await.unwrap();
    let ended = contexts.lock()[0].clone();
    let leaked_at = tokio::time::Instant::now();
    let leaked =
        ended.sql().begin_transaction(Some(LEAK)).await.unwrap();
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    host.get_or_start("keeper", "t").await.unwrap();
    if made_again {
        host.stop(&id, ShutdownKind::Destroy).await.unwrap();
        host.get_or_start("keeper", "t").await.unwrap();
    }
    assert_eq!(call(&host, &id, "count").await, 0);
    assert!(
        leaked_at.elapsed() < LEAK,
        "the transaction expired before the actor started again"
    );

    tokio::time::sleep_until(leaked_at + LEAK + PROMPT / 2).await;
    assert!(leaked.rollback().await.is_err(), "it never expired");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(host.is_running(&id), "the ended generation stopped it");
    assert_eq!(call(&host, &id, "count").await, 0);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_generations_crash_does_not_stop_the_next() {
    an_ended_generations_crash_leaves_the_actor_running(false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_generations_crash_does_not_stop_its_key_made_again()
{
    an_ended_generations_crash_leaves_the_actor_running(true).await;
}

/// A destroy asked while the actor's sleep stop waits for an action
/// makes that stop a destroy: the file is gone once both return.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_destroy_during_a_sleep_stop_removes_the_file() {
    let dir = Scratch::new("host-destroy-in-stop");
    let host = Host::open(&dir, with_nappers()).unwrap();
    let b = host.get_or_start("slow", "b").await.unwrap();
    let nap = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { act(&host, &b, "nap", Vec::new()).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let sleep = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { host.stop(&b, ShutdownKind::Sleep).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!sleep.is_finished(), "b stopped before its nap ended");

    host.stop(&b, ShutdownKind::Destroy).await.unwrap();
    sleep.await.unwrap().unwrap();
    nap.await.unwrap().unwrap();
    assert!(!host.is_running(&b));
    assert!(!dir.join("actors/slow/b.sqlite").exists());
}

/// A transaction its generation left open when the actor stops,
/// begun with a plain `BEGIN` that no transaction of the core holds:
/// the stop rolls it back and still marks the actor stopped, with the
/// deadline of the schedule its generation committed.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_ends_a_transaction_its_generation_left_open() {
    let dir = Scratch::new("host-stop-leaked");
    let contexts = Contexts::default();
    let host = Host::open(&dir, keeper(&contexts)).unwrap();
    let id = host.get_or_start("keeper", "t").await.unwrap();
    let ctx = contexts.lock()[0].clone();
    let event = ctx
        .after(Duration::from_secs(600), "count", &[])
        .await
        .unwrap();
    ctx.sql().exec("BEGIN").await.unwrap();
    ctx.sql()
        .exec("INSERT INTO marks VALUES ('leaked')")
        .await
        .unwrap();
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    drop(host);

    let conn = Connection::open_with_flags(
        dir.join("actors/keeper/t.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let (running, wake_at): (i64, Option<i64>) = conn
        .query_row(
            "SELECT running, wake_at FROM _hamlet_actor",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let due: i64 = conn
        .query_row(
            "SELECT trigger_at FROM _actor_schedule_events WHERE \
             event_id = ?",
            [&event],
            |row| row.get(0),
        )
        .unwrap();
    let marks: i64 = conn
        .query_row("SELECT COUNT(*) FROM marks", [], |row| row.get(0))
        .unwrap();
    assert_eq!((running, wake_at, marks), (0, Some(due), 0));
}

/// Whether `error` is the core's error `actor.<code>`.
fn is_actor_error(error: &anyhow::Error, code: &str) -> bool {
    let error = ActorError::extract(error);
    (error.group(), error.code()) == ("actor", code)
}

/// A call to a kind the directory does not hold fails at once with
/// `actor.not_registered`, and makes no folder or file for it.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_an_unknown_kind_fails_at_once_and_makes_no_file() {
    let dir = Scratch::new("host-call-unknown");
    let host = Host::open(&dir, factories()).unwrap();
    let error = tokio::time::timeout(
        PROMPT,
        host.call(
            &Address::new("nobody", "a"),
            "get",
            Vec::new(),
            DEADLINE,
            Cause::root("test"),
        ),
    )
    .await
    .expect("the call to an unknown kind waited")
    .expect_err("a call to an unknown kind must fail");
    assert!(is_actor_error(&error, "not_registered"), "{error:#}");
    assert!(!dir.join("actors/nobody").exists());
    assert!(!host.is_running("nobody/a"));
    host.shutdown().await.unwrap();
}

/// A call whose deadline is shorter than its action ends at the
/// deadline with `actor.action_timed_out`, though the kind's own
/// action timeout is far longer.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_ends_at_its_deadline() {
    let dir = Scratch::new("host-call-deadline");
    let host = Host::open(&dir, with_nappers()).unwrap();
    let b = host.get_or_start("slow", "b").await.unwrap();
    let begun = tokio::time::Instant::now();
    let error = host
        .call(
            &Address::new("slow", "b"),
            "nap",
            Vec::new(),
            Duration::from_millis(300),
            Cause::root("test"),
        )
        .await
        .expect_err("the nap outlived the call's deadline");
    let took = begun.elapsed();
    assert!(is_actor_error(&error, "action_timed_out"), "{error:#}");
    assert!(
        took >= Duration::from_millis(300)
            && took < Duration::from_secs(2),
        "the call ended after {took:?}"
    );
    assert!(host.is_running(&b));
    host.shutdown().await.unwrap();
}

/// A call to an actor whose stop is under way waits for that stop
/// within the call's deadline, and ends at the deadline when the stop
/// outlasts it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_waiting_for_its_actors_stop_ends_at_its_deadline() {
    let dir = Scratch::new("host-call-stop-deadline");
    let host = Host::open(&dir, with_nappers()).unwrap();
    let b = host.get_or_start("slow", "b").await.unwrap();
    let nap = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { act(&host, &b, "nap", Vec::new()).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let stop = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { host.stop(&b, ShutdownKind::Sleep).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let begun = tokio::time::Instant::now();
    let error = host
        .call(
            &Address::new("slow", "b"),
            "get",
            Vec::new(),
            Duration::from_millis(300),
            Cause::root("test"),
        )
        .await
        .expect_err("b's stop outlived the call's deadline");
    let took = begun.elapsed();
    assert!(is_actor_error(&error, "action_timed_out"), "{error:#}");
    assert!(
        took >= Duration::from_millis(300)
            && took < Duration::from_secs(2),
        "the call ended after {took:?}"
    );
    assert!(!stop.is_finished(), "b stopped before its nap ended");

    nap.await.unwrap().unwrap();
    stop.await.unwrap().unwrap();
    host.shutdown().await.unwrap();
}

/// A call to an actor that is being destroyed fails at once with
/// `actor.destroying`, without waiting for the destroy or starting
/// the actor again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_to_an_actor_being_destroyed_fails_at_once() {
    let dir = Scratch::new("host-call-destroying");
    let host = Host::open(&dir, with_nappers()).unwrap();
    let b = host.get_or_start("slow", "b").await.unwrap();
    let nap = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { act(&host, &b, "nap", Vec::new()).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let destroy = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { host.stop(&b, ShutdownKind::Destroy).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let error = tokio::time::timeout(
        PROMPT,
        act(&host, &b, "get", Vec::new()),
    )
    .await
    .expect("the call waited for the destroy")
    .expect_err("a call to an actor being destroyed must fail");
    assert!(is_actor_error(&error, "destroying"), "{error:#}");
    assert!(!destroy.is_finished(), "b was destroyed before its nap");

    nap.await.unwrap().unwrap();
    destroy.await.unwrap().unwrap();
    assert!(!host.is_running(&b));
    assert!(!dir.join("actors/slow/b.sqlite").exists());
}

/// A shutdown stops every actor at once: while one actor's stop waits
/// for a slow action, the others' files are already closed. It waits
/// for that stop too, and its error names that actor, whose stop
/// could not mark its file stopped, and no other.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shutdown_closes_the_others_while_one_is_slow_and_reports_it()
 {
    let dir = Scratch::new("host-shutdown-slow");
    let host = Host::open(&dir, with_nappers()).unwrap();
    let brittle = host.get_or_start("brittle", "b").await.unwrap();
    for key in ["a", "c"] {
        let id = host.get_or_start("counter", key).await.unwrap();
        call(&host, &id, "increment").await;
    }
    act(&host, &brittle, "unmark", Vec::new()).await.unwrap();
    let nap = tokio::spawn({
        let host = host.clone();
        let brittle = brittle.clone();
        async move { act(&host, &brittle, "nap", Vec::new()).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let shutdown = tokio::spawn({
        let host = host.clone();
        async move { host.shutdown().await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!shutdown.is_finished(), "it did not wait for the nap");
    for key in ["a", "c"] {
        assert_eq!(
            probe(&dir, "counter", key).ok(),
            Some(1),
            "counter/{key} is still open while brittle/b stops"
        );
    }

    let error = shutdown
        .await
        .unwrap()
        .expect_err("brittle/b's stop failed to mark its file");
    let text = format!("{error:#}");
    assert!(text.contains("brittle/b"), "{text}");
    assert!(!text.contains("counter/"), "{text}");
    // A call in flight fails once its host begins to shut down.
    let error = nap
        .await
        .unwrap()
        .expect_err("a call in flight as its host shuts down");
    assert!(is_actor_error(&error, "stopping"), "{error:#}");
    for id in ["brittle/b", "counter/a", "counter/c"] {
        assert!(!host.is_running(id), "{id} still runs");
    }
}

/// What a stopped counter's file holds, read with no host open.
#[derive(Debug, PartialEq)]
struct Kept {
    count: i64,
    schedules: i64,
    running: i64,
    /// Whether the file's deadline is its schedule row's.
    wakes_for_its_row: bool,
}

fn kept(dir: &Path, key: &str) -> Kept {
    let conn = Connection::open_with_flags(
        dir.join(format!("actors/counter/{key}.sqlite")),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let read = |sql: &str| -> i64 {
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    };
    Kept {
        count: read(
            "SELECT COALESCE((SELECT value FROM counter WHERE id = \
             0), 0)",
        ),
        schedules: read(
            "SELECT COUNT(*) FROM _actor_schedule_events",
        ),
        running: read("SELECT running FROM _hamlet_actor"),
        wakes_for_its_row: read(
            "SELECT wake_at = (SELECT MIN(trigger_at) FROM \
             _actor_schedule_events) FROM _hamlet_actor",
        ) == 1,
    }
}

/// A shutdown cancels the timers of stopped actors, those its own
/// stops leave included, and starts nothing after it, nor boots.
/// Their deadlines stay in their files, and the next host's boot runs
/// them.
#[tokio::test(flavor = "multi_thread")]
async fn no_timer_starts_an_actor_after_a_shutdown() {
    let dir = Scratch::new("host-shutdown-timers");
    let host = Host::open(&dir, factories()).unwrap();
    let schedule = 400u64.to_le_bytes().to_vec();
    let stopped =
        host.get_or_start("counter", "stopped").await.unwrap();
    call_with(&host, &stopped, "schedule", schedule.clone()).await;
    host.stop(&stopped, ShutdownKind::Sleep).await.unwrap();
    let running =
        host.get_or_start("counter", "running").await.unwrap();
    call_with(&host, &running, "schedule", schedule).await;

    host.shutdown().await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    for id in [&stopped, &running] {
        assert!(!host.is_running(id), "a timer started {id}");
    }
    assert!(
        act(&host, "counter/new", "get", Vec::new()).await.is_err()
    );
    assert!(!dir.join("actors/counter/new.sqlite").exists());
    assert!(host.boot().await.is_err(), "a shut host booted");
    drop(host);
    for key in ["stopped", "running"] {
        assert_eq!(
            kept(&dir, key),
            Kept {
                count: 0,
                schedules: 1,
                running: 0,
                wakes_for_its_row: true,
            },
            "counter/{key}"
        );
    }

    let host = Host::open(&dir, factories()).unwrap();
    let mut started = host.boot().await.unwrap();
    started.sort();
    assert_eq!(started, [running.clone(), stopped.clone()]);
    for id in [&stopped, &running] {
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(10);
        while call(&host, id, "get").await != 1 {
            assert!(tokio::time::Instant::now() < deadline, "{id}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    host.shutdown().await.unwrap();
}

/// Once a shutdown returns and the host is dropped, a second host
/// opens the folder at once and finds each actor as the first left
/// it, the one whose stop waited for a slow action included.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_host_opens_the_folder_at_once_after_a_shutdown() {
    let dir = Scratch::new("host-shutdown-reopen");
    let host = Host::open(&dir, with_nappers()).unwrap();
    let a = host.get_or_start("counter", "a").await.unwrap();
    call(&host, &a, "increment").await;
    let c = host.get_or_start("counter", "c").await.unwrap();
    call(&host, &c, "increment").await;
    call(&host, &c, "increment").await;
    let b = host.get_or_start("slow", "b").await.unwrap();
    let nap = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { act(&host, &b, "nap", Vec::new()).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    host.shutdown().await.unwrap();
    // A call in flight fails once its host begins to shut down.
    let error = nap
        .await
        .unwrap()
        .expect_err("a call in flight as its host shuts down");
    assert!(is_actor_error(&error, "stopping"), "{error:#}");
    drop(host);

    let again = Host::open(&dir, with_nappers())
        .expect("the first host still holds the folder");
    assert_eq!(call(&again, &a, "get").await, 1);
    assert_eq!(call(&again, &c, "get").await, 2);
    call(&again, &b, "get").await;
    again.shutdown().await.unwrap();
    for (kind, key) in
        [("counter", "a"), ("counter", "c"), ("slow", "b")]
    {
        assert_eq!(
            probe(&dir, kind, key).unwrap(),
            2,
            "{kind}/{key}"
        );
    }
}

/// Once a shutdown returns and the last `Host` is dropped, a second
/// host opens the folder, every time. Here each round's counters ask
/// to sleep as the shutdown begins, so their own stops end beside
/// it, each at its own moment, and the process starts child
/// processes all along, as a host that runs commands does.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_folder_is_free_after_every_shutdown() {
    const ROUNDS: u64 = 400;
    let dir = Scratch::new("host-shutdown-reopen-loop");
    let forking = Arc::new(AtomicBool::new(true));
    let forker = std::thread::spawn({
        let forking = forking.clone();
        move || {
            while forking.load(Ordering::Relaxed) {
                if Command::new("true").status().is_err() {
                    // No such program here: no child to start.
                    return;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    });
    let keys: Vec<String> =
        (0..16).map(|key| format!("counter/{key}")).collect();
    let mut refused = Vec::new();
    let mut host = Host::open(&dir, factories()).unwrap();
    for key in 0..16 {
        host.get_or_start("counter", &key.to_string())
            .await
            .unwrap();
    }
    for round in 0..ROUNDS {
        for id in &keys {
            act(&host, id, "get", Vec::new()).await.unwrap();
        }
        let mut sleeps = tokio::task::JoinSet::new();
        for id in keys.clone() {
            let host = host.clone();
            sleeps.spawn(async move {
                // Fails once the shutdown has begun.
                let _ = act(&host, &id, "sleep", Vec::new()).await;
            });
        }
        tokio::time::sleep(Duration::from_micros(
            round * 397 % 4_000,
        ))
        .await;
        host.shutdown().await.unwrap();
        // Only the host's own work may hold it now.
        while sleeps.join_next().await.is_some() {}
        drop(host);
        host = match Host::open(&dir, factories()) {
            Ok(again) => again,
            Err(_) => {
                refused.push(round);
                let deadline = tokio::time::Instant::now()
                    + Duration::from_secs(10);
                loop {
                    tokio::time::sleep(Duration::from_millis(10))
                        .await;
                    if let Ok(again) = Host::open(&dir, factories()) {
                        break again;
                    }
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "the folder stayed locked after round \
                         {round}"
                    );
                }
            }
        };
    }
    host.shutdown().await.unwrap();
    forking.store(false, Ordering::Relaxed);
    forker.join().unwrap();
    assert!(
        refused.is_empty(),
        "the folder was still locked right after the shutdowns of \
         rounds {refused:?} of {ROUNDS}"
    );
}

/// Once the last `Host` is dropped, the host starts no actor, though
/// a stop it began still runs and keeps the rest of it alive: here a
/// stop of `slow/y`, which waits for a nap, while the timer of
/// `counter/x` comes due. `x`'s deadline stays in its file, and the
/// next host's boot runs it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_host_starts_nothing() {
    let dir = Scratch::new("host-dropped");
    let host = Host::open(&dir, with_nappers()).unwrap();
    let x = host.get_or_start("counter", "x").await.unwrap();
    let due = 1_000u64.to_le_bytes().to_vec();
    call_with(&host, &x, "schedule", due).await;
    host.stop(&x, ShutdownKind::Sleep).await.unwrap();
    let y = host.get_or_start("slow", "y").await.unwrap();
    // The nap, and the stop that waits for it, go on without their
    // callers.
    let short = Duration::from_millis(100);
    let nap = act(&host, &y, "nap", Vec::new());
    assert!(tokio::time::timeout(short, nap).await.is_err());
    let stop = host.stop(&y, ShutdownKind::Sleep);
    assert!(tokio::time::timeout(short, stop).await.is_err());
    drop(host);

    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert_eq!(
        probe(&dir, "counter", "x")
            .map_err(|error| error.to_string()),
        Ok(1),
        "a timer started counter/x after its host was dropped"
    );
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(10);
    let again = loop {
        match Host::open(&dir, with_nappers()) {
            Ok(again) => break again,
            Err(error) => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "the stop of slow/y never let the folder go: \
                     {error:#}"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    };
    assert_eq!(again.boot().await.unwrap(), std::slice::from_ref(&x));
    while call(&again, &x, "get").await != 1 {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    again.shutdown().await.unwrap();
    assert_eq!(probe(&dir, "slow", "y").unwrap(), 1);
}

/// A start deadline short enough for a test.
const START_DEADLINE: Duration = Duration::from_millis(500);

/// Sets its flag when dropped.
struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// The kind `kind`, whose start never ends: its run handler never
/// signals that it is ready. Its run future sets `dropped` when it is
/// dropped.
fn stuck(
    kind: &str,
    dropped: &Arc<AtomicBool>,
) -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some(kind.to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    let dropped = dropped.clone();
    HashMap::from([(
        kind.to_owned(),
        ActorFactory::new_with_manual_startup_ready(
            config,
            move |start: ActorStart| {
                let flag = DropFlag(dropped.clone());
                Box::pin(async move {
                    let _held = (flag, start);
                    std::future::pending::<()>().await;
                    Ok(())
                })
            },
        ),
    )])
}

/// Waits up to a second for `dropped` to be set.
async fn wait_for_the_drop(dropped: &AtomicBool) {
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(1);
    while !dropped.load(Ordering::SeqCst) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the run future was not dropped"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A start that never finishes fails at its deadline with
/// `actor.not_ready`, and closes the actor's file, which keeps its
/// running mark. A call and a stop that came during the start end
/// with it, without a start of their own: the call with the start's
/// error, the stop with nothing to stop. The run handler's future is
/// dropped.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_that_never_finishes_fails_at_its_deadline() {
    let dir = Scratch::new("host-stuck-start");
    let dropped = Arc::new(AtomicBool::new(false));
    let host = Host::open_with(
        &dir,
        stuck("stuck", &dropped),
        HostOptions {
            start_deadline: START_DEADLINE,
            ..HostOptions::default()
        },
    )
    .unwrap();
    let began = tokio::time::Instant::now();
    let start = tokio::spawn({
        let host = host.clone();
        async move { host.get_or_start("stuck", "a").await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let call = tokio::spawn({
        let host = host.clone();
        async move {
            host.call(
                &Address::new("stuck", "a"),
                "get",
                Vec::new(),
                Duration::from_secs(30),
                Cause::root("test"),
            )
            .await
        }
    });
    let stop = tokio::spawn({
        let host = host.clone();
        async move { host.stop("stuck/a", ShutdownKind::Sleep).await }
    });
    let by = began + START_DEADLINE * 3;
    let start = tokio::time::timeout_at(by, start)
        .await
        .expect("the start outlived its deadline")
        .unwrap()
        .expect_err("a start that never finishes must fail");
    assert!(is_actor_error(&start, "not_ready"), "{start:#}");
    let call = tokio::time::timeout_at(by, call)
        .await
        .expect("the call outlived the start")
        .unwrap()
        .expect_err("the call must fail with the start");
    assert!(is_actor_error(&call, "not_ready"), "{call:#}");
    tokio::time::timeout_at(by, stop)
        .await
        .expect("the stop outlived the start")
        .unwrap()
        .unwrap();
    assert!(!host.is_running("stuck/a"));
    let running = Connection::open_with_flags(
        dir.join("actors/stuck/a.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .and_then(|conn| {
        conn.query_row(
            "SELECT running FROM _hamlet_actor",
            [],
            |row| row.get::<_, i64>(0),
        )
    });
    assert_eq!(running.map_err(|error| error.to_string()), Ok(1));
    wait_for_the_drop(&dropped).await;
    host.shutdown().await.unwrap();
}

/// No call waits on a host that is shutting down, which starts no
/// actor again: a call that waits for its actor's stop when the
/// shutdown begins fails then, and one that comes later fails at
/// once, each with `actor.stopping`, while the stop waits for a nap.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_fails_at_once_once_its_host_shuts_down() {
    let dir = Scratch::new("host-shut-call");
    let host = Host::open(&dir, with_nappers()).unwrap();
    let b = host.get_or_start("slow", "b").await.unwrap();
    let get = |host: &Host| {
        let host = host.clone();
        let b = b.clone();
        tokio::spawn(async move {
            act(&host, &b, "get", Vec::new()).await
        })
    };
    let nap = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { act(&host, &b, "nap", Vec::new()).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let stop = tokio::spawn({
        let host = host.clone();
        let b = b.clone();
        async move { host.stop(&b, ShutdownKind::Sleep).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let waiting = get(&host);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !waiting.is_finished(),
        "the call did not wait for the stop"
    );

    let shutdown = tokio::spawn({
        let host = host.clone();
        async move { host.shutdown().await }
    });
    let short = Duration::from_millis(300);
    for call in [waiting, get(&host)] {
        let error = tokio::time::timeout(short, call)
            .await
            .expect("a call waited on a host that is shutting down")
            .unwrap()
            .expect_err("a call to a host that is shutting down");
        assert!(is_actor_error(&error, "stopping"), "{error:#}");
    }
    // A call in flight fails once its host begins to shut down.
    let error = nap
        .await
        .unwrap()
        .expect_err("a call in flight as its host shuts down");
    assert!(is_actor_error(&error, "stopping"), "{error:#}");
    stop.await.unwrap().unwrap();
    shutdown.await.unwrap().unwrap();
}

/// A shutdown does not wait out a stuck start's deadline: it ends the
/// start after a short grace.
#[tokio::test(flavor = "multi_thread")]
async fn a_shutdown_returns_while_a_start_is_stuck() {
    let dir = Scratch::new("host-stuck-shutdown");
    let dropped = Arc::new(AtomicBool::new(false));
    let host = Host::open(&dir, stuck("stuck", &dropped)).unwrap();
    let start = tokio::spawn({
        let host = host.clone();
        async move { host.get_or_start("stuck", "a").await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    tokio::time::timeout(Duration::from_secs(3), host.shutdown())
        .await
        .expect("the shutdown waited for the stuck start")
        .unwrap();
    let start = tokio::time::timeout(PROMPT, start)
        .await
        .expect("the start outlived the shutdown")
        .unwrap()
        .expect_err("a start the shutdown ended must fail");
    assert!(is_actor_error(&start, "not_ready"), "{start:#}");
    assert!(!host.is_running("stuck/a"));
    wait_for_the_drop(&dropped).await;
}

/// Boot begins the starts of the actors a kill left running and
/// returns before any of them ends, also when none ever ends. Each
/// fails at its own deadline, and a call that waited for one fails
/// with it.
#[tokio::test(flavor = "multi_thread")]
async fn boot_returns_while_the_starts_it_began_are_stuck() {
    let dir = Scratch::new("host-stuck-boot");
    let keys = ["a", "b", "c"];
    let host = Host::open(&dir, factories()).unwrap();
    for key in keys {
        host.get_or_start("counter", key).await.unwrap();
    }
    host.shutdown().await.unwrap();
    drop(host);
    // As a kill leaves them.
    for key in keys {
        Connection::open(
            dir.join(format!("actors/counter/{key}.sqlite")),
        )
        .unwrap()
        .execute("UPDATE _hamlet_actor SET running = 1", [])
        .unwrap();
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let host = Host::open_with(
        &dir,
        stuck("counter", &dropped),
        HostOptions {
            start_deadline: START_DEADLINE,
            ..HostOptions::default()
        },
    )
    .unwrap();
    let mut began = tokio::time::timeout(START_DEADLINE, host.boot())
        .await
        .expect("boot waited for a stuck start")
        .unwrap();
    began.sort();
    assert_eq!(began, keys.map(|key| format!("counter/{key}")));
    for id in &began {
        let error =
            act(&host, id, "get", Vec::new()).await.unwrap_err();
        assert!(is_actor_error(&error, "not_ready"), "{error:#}");
    }
    host.shutdown().await.unwrap();
}

/// H1. Boot leaves alone the file of an actor its host runs, which
/// the actor holds locked.
#[tokio::test(flavor = "multi_thread")]
async fn boot_succeeds_while_an_actor_of_its_host_runs() {
    let dir = Scratch::new("host-boot-running");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    assert!(host.boot().await.unwrap().is_empty());
    assert_eq!(call(&host, &id, "increment").await, 1);
    host.shutdown().await.unwrap();
}

/// How long the start of a [`slow_counters`] actor takes.
const SLOW_START: Duration = Duration::from_secs(5);

/// The demo counter, whose start ends [`SLOW_START`] after its run
/// handler begins.
fn slow_counters() -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("counter".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    HashMap::from([(
        "counter".to_owned(),
        ActorFactory::new_with_manual_startup_ready(
            config,
            |mut start: ActorStart| {
                Box::pin(async move {
                    let ready = start.startup_ready.take();
                    tokio::time::sleep(SLOW_START).await;
                    if let Some(ready) = ready {
                        let _ = ready.send(Ok(()));
                    }
                    counter::run(start).await
                })
            },
        ),
    )])
}

/// L7. Boot begins the start of an actor a kill cut off, and returns
/// without waiting for it. A call made meanwhile waits for the start.
#[tokio::test(flavor = "multi_thread")]
async fn boot_returns_before_the_starts_it_begins() {
    let dir = Scratch::new("host-boot-returns");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    call(&host, &id, "increment").await;
    host.shutdown().await.unwrap();
    drop(host);
    // As a kill leaves it.
    Connection::open(dir.join("actors/counter/a.sqlite"))
        .unwrap()
        .execute("UPDATE _hamlet_actor SET running = 1", [])
        .unwrap();

    let host = Host::open(&dir, slow_counters()).unwrap();
    let begun = tokio::time::Instant::now();
    let began =
        tokio::time::timeout(Duration::from_secs(1), host.boot())
            .await
            .expect("boot waited for the start it began")
            .unwrap();
    assert_eq!(began, std::slice::from_ref(&id));
    assert!(!host.is_running(&id));
    assert_eq!(call(&host, &id, "get").await, 1);
    assert!(begun.elapsed() >= SLOW_START);
    host.shutdown().await.unwrap();
}

/// What is logged on the thread that made it, at WARN or above.
#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Logs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// L1. Boot reads only the files of its own kinds. Another kind's
/// file, which another process holds locked with a run a kill cut
/// off, neither fails nor slows it, nor is logged, and nothing of
/// that kind starts.
#[tokio::test(flavor = "multi_thread")]
async fn boot_reads_only_the_files_of_its_kinds() {
    use tracing_subscriber::util::SubscriberInitExt;

    let dir = Scratch::new("host-boot-kinds");
    std::fs::create_dir_all(dir.join("actors/other")).unwrap();
    let held =
        Connection::open(dir.join("actors/other/x.sqlite")).unwrap();
    held.pragma_update(None, "locking_mode", "EXCLUSIVE")
        .unwrap();
    held.execute_batch(
        "CREATE TABLE _hamlet_actor (id INTEGER PRIMARY KEY, kind \
         TEXT NOT NULL, key TEXT NOT NULL, generation INTEGER NOT \
         NULL, wake_at INTEGER, running INTEGER NOT NULL); INSERT \
         INTO _hamlet_actor VALUES (0, 'other', 'x', 1, NULL, 1)",
    )
    .unwrap();
    // The write took the file's exclusive lock, which `held` keeps.
    assert!(probe(&dir, "other", "x").is_err());

    let host = Host::open(&dir, factories()).unwrap();
    let logs = Logs::default();
    let logging = tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::WARN)
        .with_ansi(false)
        .with_writer({
            let logs = logs.clone();
            move || logs.clone()
        })
        .finish()
        .set_default();
    let began =
        tokio::time::timeout(Duration::from_secs(1), host.boot())
            .await
            .expect("another kind's file slowed boot")
            .unwrap();
    drop(logging);
    assert!(began.is_empty(), "{began:?}");
    let logged = String::from_utf8(logs.0.lock().clone()).unwrap();
    assert!(logged.is_empty(), "{logged}");
    host.shutdown().await.unwrap();
    drop(host);
    drop(held);
    assert_eq!(probe(&dir, "other", "x").unwrap(), 1);
}

/// L2. Hosts of different names, each with a kind of its own, open
/// one folder at once, and each boots while the other runs an actor.
/// A second host of one name is refused by that name's lock, and a
/// name must be a plain file name.
#[tokio::test(flavor = "multi_thread")]
async fn hosts_of_different_names_share_a_folder() {
    let dir = Scratch::new("host-names");
    let named = |name: &str| HostOptions {
        name: name.to_owned(),
        ..HostOptions::default()
    };
    let others = || {
        let config = ActorConfig {
            name: Some("other".to_owned()),
            has_database: true,
            ..ActorConfig::default()
        };
        HashMap::from([(
            "other".to_owned(),
            ActorFactory::new(config, |start| {
                Box::pin(counter::run(start))
            }),
        )])
    };
    let main =
        Host::open_with(&dir, factories(), named("main")).unwrap();
    let a = main.get_or_start("counter", "a").await.unwrap();
    let node =
        Host::open_with(&dir, others(), named("node")).unwrap();
    let x = node.get_or_start("other", "x").await.unwrap();
    assert!(main.boot().await.unwrap().is_empty());
    assert!(node.boot().await.unwrap().is_empty());

    let Err(error) = Host::open_with(&dir, others(), named("node"))
    else {
        panic!("a second host named node opened the folder");
    };
    assert!(error.to_string().contains("node.lock"), "{error:#}");
    assert!(
        Host::open_with(&dir, others(), named("../node")).is_err()
    );

    assert_eq!(call(&main, &a, "increment").await, 1);
    assert_eq!(call(&node, &x, "increment").await, 1);
    main.shutdown().await.unwrap();
    node.shutdown().await.unwrap();
}

/// E1. One actor file that boot cannot read does not fail the boot.
/// Boot logs it once, raises an event that names its actor, and
/// boots the rest: a counter whose one-shot came due while the host
/// was down starts and runs it.
#[tokio::test(flavor = "multi_thread")]
async fn boot_goes_past_a_file_it_cannot_read() {
    use tracing_subscriber::util::SubscriberInitExt;

    let dir = Scratch::new("host-boot-unreadable");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    call_with(&host, &id, "schedule", 100u64.to_le_bytes().to_vec())
        .await;
    host.shutdown().await.unwrap();
    drop(host);
    std::fs::write(
        dir.join("actors/counter/bad.sqlite"),
        b"not a database, only bytes".repeat(64),
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let host = Host::open(&dir, factories()).unwrap();
    let mut events = host.events();
    let logs = Logs::default();
    let logging = tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::WARN)
        .with_ansi(false)
        .with_writer({
            let logs = logs.clone();
            move || logs.clone()
        })
        .finish()
        .set_default();
    let began = host.boot().await;
    drop(logging);
    assert_eq!(began.unwrap(), std::slice::from_ref(&id));
    let logged = String::from_utf8(logs.0.lock().clone()).unwrap();
    assert_eq!(logged.matches("bad.sqlite").count(), 1, "{logged}");
    let event = events.try_recv().expect("no event for the file");
    assert!(
        matches!(
            &event,
            HostEvent::UnreadableFile { actor, .. }
                if *actor == Address::new("counter", "bad")
        ),
        "{event:?}"
    );
    assert!(events.try_recv().is_err(), "a second event");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(call(&host, &id, "get").await, 1);
    host.shutdown().await.unwrap();
}

/// A file that a destroy removed after boot listed the files is not
/// one boot cannot read: boot passes over it and raises no event.
/// Each round makes `KEYS` counters that stopped cleanly, then boots
/// a host while it destroys every one of them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn boot_passes_over_a_file_a_destroy_removed_after_its_list() {
    const KEYS: usize = 32;
    const ROUNDS: usize = 20;
    let dir = Scratch::new("host-boot-destroy-race");
    let keys: Vec<String> =
        (0..KEYS).map(|n| format!("k{n}")).collect();
    for round in 0..ROUNDS {
        let host = Host::open(&dir, factories()).unwrap();
        for key in &keys {
            host.get_or_start("counter", key).await.unwrap();
        }
        host.shutdown().await.unwrap();
        drop(host);

        let host = Host::open(&dir, factories()).unwrap();
        let mut events = host.events();
        let boot = tokio::spawn({
            let host = host.clone();
            async move { host.boot().await }
        });
        let destroys: Vec<_> = keys
            .iter()
            .map(|key| {
                let host = host.clone();
                let id = format!("counter/{key}");
                tokio::spawn(async move {
                    host.stop(&id, ShutdownKind::Destroy).await
                })
            })
            .collect();
        for destroy in destroys {
            destroy.await.unwrap().unwrap();
        }
        assert!(boot.await.unwrap().unwrap().is_empty());
        if let Ok(event) = events.try_recv() {
            panic!("round {round}: boot raised {event:?}");
        }
        for key in &keys {
            assert!(!any_file_of(&dir, "counter", key), "{key}");
        }
        host.shutdown().await.unwrap();
    }
}

/// A start that begins while boot reads the same actor's file waits
/// for the read, and never fails on the file's lock. Each round
/// boots a host over `KEYS` counters that stopped cleanly, so boot
/// reads every file and starts none, and starts every counter while
/// boot runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_start_during_boots_read_of_its_file_waits_for_the_read() {
    const KEYS: usize = 8;
    const ROUNDS: usize = 100;
    let dir = Scratch::new("host-boot-read-race");
    let keys: Vec<String> =
        (0..KEYS).map(|n| format!("k{n}")).collect();
    let host = Host::open(&dir, factories()).unwrap();
    for key in &keys {
        host.get_or_start("counter", key).await.unwrap();
    }
    host.shutdown().await.unwrap();
    drop(host);

    for round in 0..ROUNDS {
        let host = Host::open(&dir, factories()).unwrap();
        let boot = tokio::spawn({
            let host = host.clone();
            async move { host.boot().await }
        });
        let starts: Vec<_> = keys
            .iter()
            .map(|key| {
                let host = host.clone();
                let key = key.clone();
                tokio::spawn(async move {
                    host.get_or_start("counter", &key).await
                })
            })
            .collect();
        for start in starts {
            if let Err(error) = start.await.unwrap() {
                panic!("round {round}: a start failed: {error:#}");
            }
        }
        assert!(boot.await.unwrap().unwrap().is_empty());
        host.shutdown().await.unwrap();
    }
}

/// Hosts of different names cannot both hold one kind: a host given a
/// kind that another host holds fails to open, and names that kind's
/// lock. The kind is free once its host has shut down.
#[tokio::test(flavor = "multi_thread")]
async fn hosts_of_different_names_cannot_share_a_kind() {
    let dir = Scratch::new("host-shared-kind");
    let named = |name: &str| HostOptions {
        name: name.to_owned(),
        ..HostOptions::default()
    };
    let main =
        Host::open_with(&dir, factories(), named("main")).unwrap();
    let Err(error) =
        Host::open_with(&dir, factories(), named("node"))
    else {
        panic!("a second host of the kind counter opened the folder");
    };
    let lock = dir.join("actors").join("counter").join(".lock");
    assert!(
        error.to_string().contains(&lock.display().to_string()),
        "{error:#}"
    );
    main.shutdown().await.unwrap();
    let node =
        Host::open_with(&dir, factories(), named("node")).unwrap();
    node.shutdown().await.unwrap();
}

/// Whether any of the actor's three files is on disk: its file, its
/// write-ahead file and its shared-memory file.
fn any_file_of(dir: &Path, kind: &str, key: &str) -> bool {
    ["", "-wal", "-shm"].iter().any(|suffix| {
        dir.join("actors")
            .join(kind)
            .join(format!("{key}.sqlite{suffix}"))
            .exists()
    })
}

/// A destroy removes the actor's data whether it runs, sleeps with
/// no deadline, or sleeps waiting for a timer, whose timer then never
/// starts it. A destroy repeated afterwards ends the same way, and a
/// new start of each key reads nothing of the old one.
#[tokio::test(flavor = "multi_thread")]
async fn a_destroy_removes_a_running_a_sleeping_and_a_waiting_actor()
{
    let dir = Scratch::new("host-destroy-states");
    let host = Host::open(&dir, factories()).unwrap();
    for key in ["running", "sleeping", "waiting"] {
        let id = host.get_or_start("counter", key).await.unwrap();
        call(&host, &id, "increment").await;
    }
    call_with(
        &host,
        "counter/waiting",
        "schedule",
        300u64.to_le_bytes().to_vec(),
    )
    .await;
    for key in ["sleeping", "waiting"] {
        host.stop(&format!("counter/{key}"), ShutdownKind::Sleep)
            .await
            .unwrap();
    }

    for key in ["running", "sleeping", "waiting"] {
        let id = format!("counter/{key}");
        host.stop(&id, ShutdownKind::Destroy).await.unwrap();
        assert!(!host.is_running(&id), "{key}");
        assert!(!any_file_of(&dir, "counter", key), "{key}");
        host.stop(&id, ShutdownKind::Destroy).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!host.is_running("counter/waiting"));
    assert!(!any_file_of(&dir, "counter", "waiting"));
    host.shutdown().await.unwrap();
    drop(host);

    let host = Host::open(&dir, factories()).unwrap();
    assert!(host.boot().await.unwrap().is_empty());
    for key in ["running", "sleeping", "waiting"] {
        host.get_or_start("counter", key).await.unwrap();
        assert_eq!(
            call(&host, &format!("counter/{key}"), "get").await,
            0
        );
    }
    host.shutdown().await.unwrap();
}

/// A destroy reaches only a valid id of one of the host's own kinds,
/// in a host that has not shut down: it removes no other file.
#[tokio::test(flavor = "multi_thread")]
async fn a_destroy_refuses_an_id_that_is_not_its_hosts() {
    let dir = Scratch::new("host-destroy-refused");
    std::fs::create_dir_all(dir.join("actors/other")).unwrap();
    std::fs::write(dir.join("actors/other/x.sqlite"), b"kept")
        .unwrap();
    std::fs::write(dir.join("kept.sqlite"), b"kept").unwrap();
    let host = Host::open(&dir, factories()).unwrap();
    for id in
        ["other/x", "counter/../../kept", "counter", "counter/.x"]
    {
        assert!(
            host.stop(id, ShutdownKind::Destroy).await.is_err(),
            "{id}"
        );
    }
    let id = host.get_or_start("counter", "a").await.unwrap();
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    host.shutdown().await.unwrap();
    assert!(host.stop(&id, ShutdownKind::Destroy).await.is_err());
    assert!(dir.join("actors/other/x.sqlite").exists());
    assert!(dir.join("kept.sqlite").exists());
    assert!(dir.join("actors/counter/a.sqlite").exists());
}

/// A destroy of an actor whose generation left a transaction open
/// does not wait for that transaction: the stop rolls it back, and
/// the destroy removes the actor's files.
#[tokio::test(flavor = "multi_thread")]
async fn a_destroy_ends_a_transaction_its_generation_left_open() {
    let dir = Scratch::new("host-destroy-leaked");
    let contexts = Contexts::default();
    let host = Host::open(&dir, keeper(&contexts)).unwrap();
    let id = host.get_or_start("keeper", "t").await.unwrap();
    let ctx = contexts.lock()[0].clone();
    ctx.sql().exec("BEGIN").await.unwrap();
    ctx.sql()
        .exec("INSERT INTO marks VALUES ('leaked')")
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(30),
        host.stop(&id, ShutdownKind::Destroy),
    )
    .await
    .expect("the destroy waited for the open transaction")
    .unwrap();
    assert!(!any_file_of(&dir, "keeper", "t"));
    host.shutdown().await.unwrap();
}

/// A file that holds a destroy, as a kill leaves one, is removed at
/// the actor's next start: `get_or_start` makes the actor anew, and a
/// start that makes no file fails as for a missing one.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_finishes_the_destroy_its_file_holds() {
    let dir = Scratch::new("host-destroy-stored");
    let host = Host::open(&dir, factories()).unwrap();
    for key in ["made", "kept-out"] {
        let id = host.get_or_start("counter", key).await.unwrap();
        call(&host, &id, "increment").await;
        host.stop(&id, ShutdownKind::Sleep).await.unwrap();
        Connection::open(
            dir.join(format!("actors/counter/{key}.sqlite")),
        )
        .unwrap()
        .execute("UPDATE _hamlet_actor SET destroying = 1", [])
        .unwrap();
    }

    host.get_or_start("counter", "made").await.unwrap();
    assert_eq!(call(&host, "counter/made", "get").await, 0);
    let kept_out = Address::new("counter", "kept-out");
    let error = host
        .set_aside_timers(&kept_out)
        .await
        .expect_err("a start of a destroyed actor's file");
    assert!(is_actor_error(&error, "not_found"), "{error:#}");
    assert!(!any_file_of(&dir, "counter", "kept-out"));
    host.shutdown().await.unwrap();
}
