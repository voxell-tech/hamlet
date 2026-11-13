//! Timers: what a one-shot schedule does when its handler panics,
//! and when its actor sleeps or is destroyed as it comes due, and a
//! stored deadline at either end of the clock's range.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use act::act;
use anyhow::Result;
use async_trait::async_trait;
use hamlet_actor::{
    Action, Actor, ActorConfig, Ctx, Handler, Registry,
};
use hamlet_actor_core::{ActorFactory, ShutdownKind};
use hamlet_host::Host;
use hamlet_testing::Scratch;
use serde::{Deserialize, Serialize};
use tokio::time::{Instant, sleep};

#[path = "../examples/demo/act.rs"]
mod act;
#[path = "../examples/demo/counter.rs"]
mod counter;

/// How far ahead a test schedules its one-shot.
const SOON_MS: u64 = 10;

/// What one actor's file holds, read with its file closed.
#[derive(Debug, PartialEq)]
struct Stored {
    runs: i64,
    schedules: i64,
}

/// Reads the actor's file. `runs` is the one row of its table
/// `table`.
fn stored(dir: &Path, actor_id: &str, table: &str) -> Stored {
    let conn = rusqlite::Connection::open_with_flags(
        dir.join(format!("actors/{actor_id}.sqlite")),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let read = |sql: &str| -> i64 {
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    };
    Stored {
        runs: read(&format!(
            "SELECT COALESCE((SELECT value FROM {table} WHERE id = \
             0), 0)"
        )),
        schedules: read(
            "SELECT COUNT(*) FROM _actor_schedule_events",
        ),
    }
}

/// Calls `action` until `runs` reads its reply as `want`.
async fn wait_for_runs(
    host: &Host,
    actor_id: &str,
    action: &str,
    runs: impl Fn(Vec<u8>) -> i64,
    want: i64,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let reply =
            act(host, actor_id, action, Vec::new()).await.unwrap();
        if runs(reply) == want {
            return;
        }
        assert!(Instant::now() < deadline, "never ran {want} times");
        sleep(Duration::from_millis(10)).await;
    }
}

/// Schedules the actor's one-shot handler, which records its run and
/// panics. Checks that it ran once and that the panic ended its row,
/// then starts the actor again and checks it does not run again.
async fn a_panicking_handler_runs_once(
    dir: &Path,
    factories: impl Fn() -> HashMap<String, ActorFactory>,
    kind: &str,
    schedule: (&str, Vec<u8>),
    read_runs: (&str, fn(Vec<u8>) -> i64),
    table: &str,
) {
    let (schedule, args) = schedule;
    let (get, runs) = read_runs;
    let host = Host::open(dir, factories()).unwrap();
    let id = host.get_or_start(kind, "a").await.unwrap();
    act(&host, &id, schedule, args).await.unwrap();
    wait_for_runs(&host, &id, get, runs, 1).await;
    // The stop waits for the scheduled dispatch to finish.
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    sleep(Duration::from_millis(300)).await;
    assert!(!host.is_running(&id), "woken again after its stop");
    drop(host);
    assert_eq!(
        stored(dir, &id, table),
        Stored {
            runs: 1,
            schedules: 0
        },
        "the panic kept the one-shot"
    );

    let host = Host::open(dir, factories()).unwrap();
    host.get_or_start(kind, "a").await.unwrap();
    // Long enough for a start to run an overdue one-shot.
    sleep(Duration::from_millis(500)).await;
    wait_for_runs(&host, &id, get, runs, 1).await;
    host.shutdown().await.unwrap();
}

/// The raw event API: the demo counter's `panic` action adds one and
/// panics, in the task that holds its reply.
#[tokio::test(flavor = "multi_thread")]
async fn a_one_shot_whose_raw_handler_panics_runs_once() {
    let dir = Scratch::new("timer-panic-raw");
    let mut args = SOON_MS.to_le_bytes().to_vec();
    args.extend_from_slice(b"panic");
    a_panicking_handler_runs_once(
        &dir,
        || counter::factories(counter::Sleep::WhenIdle),
        "counter",
        ("schedule", args),
        ("get", |reply| counter::count(reply).unwrap()),
        "counter",
    )
    .await;
}

/// An actor of the authoring layer's trait API. `arm` schedules a
/// one-shot `fire`, which records its run in the table `runs` and
/// panics; `runs` reads the record.
struct Fuse;

#[derive(Serialize, Deserialize)]
struct Arm;

#[derive(Serialize, Deserialize)]
struct Fire;

#[derive(Serialize, Deserialize)]
struct Runs;

impl Action for Arm {
    type Output = ();
    const NAME: &'static str = "arm";
}

impl Action for Fire {
    type Output = ();
    const NAME: &'static str = "fire";
}

impl Action for Runs {
    type Output = i64;
    const NAME: &'static str = "runs";
}

#[async_trait]
impl Actor for Fuse {
    type Input = ();
    type Actions = (Arm, Fire, Runs);
    type Events = ();
    type ConnParams = ();
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        ctx.sql()
            .exec(
                "CREATE TABLE IF NOT EXISTS runs (id INTEGER \
                 PRIMARY KEY, value INTEGER NOT NULL)",
            )
            .await?;
        Ok(Self)
    }
}

impl Handler<Arm> for Fuse {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Arm,
    ) -> Result<()> {
        ctx.schedule()
            .after(Duration::from_millis(SOON_MS), Fire::NAME, &[])
            .await?;
        Ok(())
    }
}

impl Handler<Fire> for Fuse {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Fire,
    ) -> Result<()> {
        ctx.sql()
            .execute(
                "INSERT INTO runs (id, value) VALUES (0, 1) ON \
                 CONFLICT (id) DO UPDATE SET value = value + 1",
                None,
            )
            .await?;
        panic!("`fire` panics");
    }
}

impl Handler<Runs> for Fuse {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Runs,
    ) -> Result<i64> {
        let result = ctx
            .sql()
            .query("SELECT COALESCE(MAX(value), 0) FROM runs", None)
            .await?;
        match result.rows.first().and_then(|row| row.first()) {
            Some(hamlet_actor::ColumnValue::Integer(runs)) => {
                Ok(*runs)
            }
            other => anyhow::bail!("runs read {other:?}"),
        }
    }
}

fn fuse_factories() -> HashMap<String, ActorFactory> {
    let mut registry = Registry::new();
    registry.register_actor_with::<Fuse>(
        "fuse",
        ActorConfig {
            name: Some("fuse".to_owned()),
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry.into_factories()
}

/// The trait API: `fire` panics in the task the authoring layer runs
/// it on, which holds its reply.
#[tokio::test(flavor = "multi_thread")]
async fn a_one_shot_whose_trait_handler_panics_runs_once() {
    let dir = Scratch::new("timer-panic-trait");
    a_panicking_handler_runs_once(
        &dir,
        fuse_factories,
        "fuse",
        ("arm", Vec::new()),
        ("runs", |reply| ciborium::from_reader(&reply[..]).unwrap()),
        "runs",
    )
    .await;
}

/// The due time of each one-shot in a sweep.
const SWEEP_DUE_MS: u64 = 100;

/// Starts one counter per step, `<prefix><n>`. Each schedules a
/// one-shot `increment` and is stopped for `reason` at its own moment
/// near the one-shot's due time, in steps of 100 µs from 3 ms before
/// to 3 ms after. Returns the actors' ids.
async fn stop_each_near_its_due_time(
    host: &Host,
    prefix: &str,
    reason: ShutdownKind,
) -> Vec<String> {
    let mut ids = Vec::new();
    for step in -30i64..=30 {
        let key = format!("{prefix}{}", step + 30);
        let id = host.get_or_start("counter", &key).await.unwrap();
        let scheduled = std::time::Instant::now();
        act(
            host,
            &id,
            "schedule",
            SWEEP_DUE_MS.to_le_bytes().to_vec(),
        )
        .await
        .unwrap();
        let micros = SWEEP_DUE_MS as i64 * 1000 + step * 100;
        let at = scheduled
            + Duration::from_micros(u64::try_from(micros).unwrap());
        // Spins, so the stop lands within microseconds of `at`.
        tokio::task::block_in_place(|| {
            while std::time::Instant::now() < at {
                std::hint::spin_loop();
            }
        });
        host.stop(&id, reason).await.unwrap();
        ids.push(id);
    }
    ids
}

/// A1. Each counter sleeps near the time its one-shot comes due,
/// before, while or after it runs. However the stop and the one-shot
/// fall, the one-shot runs once: a stop that comes first leaves a
/// deadline that wakes the actor for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sleep_near_the_due_time_keeps_the_timer() {
    let dir = Scratch::new("timer-sleep-sweep");
    let factories = || counter::factories(counter::Sleep::WhenIdle);
    let host = Host::open(&dir, factories()).unwrap();
    let ids =
        stop_each_near_its_due_time(&host, "s", ShutdownKind::Sleep)
            .await;
    // Every deadline is past by now; its actor wakes at once.
    sleep(Duration::from_secs(1)).await;
    host.shutdown().await.unwrap();
    drop(host);

    let stranded: Vec<(&String, Stored)> = ids
        .iter()
        .map(|id| (id, stored(&dir, id, "counter")))
        .filter(|(_, stored)| {
            *stored
                != Stored {
                    runs: 1,
                    schedules: 0,
                }
        })
        .collect();
    assert!(
        stranded.is_empty(),
        "one-shots not run once: {stranded:?}"
    );
}

/// A2. Each counter is destroyed near the time its one-shot comes
/// due. No timer brings a destroyed actor back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_alarm_due_during_a_destroy_does_not_revive_it() {
    let dir = Scratch::new("timer-destroy-sweep");
    let factories = || counter::factories(counter::Sleep::WhenIdle);
    let host = Host::open(&dir, factories()).unwrap();
    let ids = stop_each_near_its_due_time(
        &host,
        "d",
        ShutdownKind::Destroy,
    )
    .await;
    sleep(Duration::from_secs(1)).await;
    let file = |id: &String| dir.join(format!("actors/{id}.sqlite"));
    let revived: Vec<&String> = ids
        .iter()
        .filter(|id| host.is_running(id) || file(id).exists())
        .collect();
    assert!(revived.is_empty(), "destroyed actors back: {revived:?}");
    host.shutdown().await.unwrap();
    drop(host);
    let left: Vec<&String> =
        ids.iter().filter(|id| file(id).exists()).collect();
    assert!(left.is_empty(), "destroyed actors' files: {left:?}");
}

/// A timer starts only an actor whose file is still there: a timer
/// never makes a file.
#[tokio::test(flavor = "multi_thread")]
async fn a_timer_does_not_make_a_file_that_is_gone() {
    let dir = Scratch::new("timer-file-gone");
    let host = Host::open(
        &dir,
        counter::factories(counter::Sleep::WhenIdle),
    )
    .unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    act(&host, &id, "schedule", 300u64.to_le_bytes().to_vec())
        .await
        .unwrap();
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    let file = dir.join(format!("actors/{id}.sqlite"));
    std::fs::remove_file(&file).unwrap();
    sleep(Duration::from_millis(700)).await;
    assert!(!host.is_running(&id));
    assert!(!file.exists(), "the timer made the file again");
    host.shutdown().await.unwrap();
}

/// Stores `wake_at` as the deadline of a stopped counter, as its last
/// stop would, and boots a host on the folder. Returns the host, the
/// counter's id and what boot started. Fails if boot does not return
/// within five seconds.
async fn boot_with_deadline(
    dir: &Path,
    wake_at: i64,
) -> (Host, String, Vec<String>) {
    let factories = || counter::factories(counter::Sleep::WhenIdle);
    let host = Host::open(dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    host.shutdown().await.unwrap();
    drop(host);
    rusqlite::Connection::open(
        dir.join(format!("actors/{id}.sqlite")),
    )
    .unwrap()
    .execute("UPDATE _hamlet_actor SET wake_at = ?", [wake_at])
    .unwrap();
    let host = Host::open(dir, factories()).unwrap();
    let started =
        tokio::time::timeout(Duration::from_secs(5), host.boot())
            .await
            .expect("boot waits for good on a due deadline")
            .unwrap();
    (host, id, started)
}

/// A deadline at the lowest time is past, so boot starts its actor at
/// once.
#[tokio::test(flavor = "multi_thread")]
async fn a_deadline_at_the_lowest_time_starts_the_actor_at_once() {
    let dir = Scratch::new("timer-deadline-min");
    let (host, id, started) =
        boot_with_deadline(&dir, i64::MIN).await;
    assert_eq!(started, vec![id.clone()]);
    assert!(host.is_running(&id));
    host.shutdown().await.unwrap();
}

/// A deadline at the highest time never comes due: boot starts
/// nothing, and a call still starts the actor.
#[tokio::test(flavor = "multi_thread")]
async fn a_deadline_at_the_highest_time_waits() {
    let dir = Scratch::new("timer-deadline-max");
    let (host, id, started) =
        boot_with_deadline(&dir, i64::MAX).await;
    assert!(started.is_empty());
    assert!(!host.is_running(&id));
    let reply = act(&host, &id, "get", Vec::new()).await.unwrap();
    assert_eq!(counter::count(reply).unwrap(), 0);
    host.shutdown().await.unwrap();
}
