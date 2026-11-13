//! Timers: what a one-shot schedule does when its handler panics.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

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
            host.action(actor_id, action, Vec::new()).await.unwrap();
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
    host.action(&id, schedule, args).await.unwrap();
    wait_for_runs(&host, &id, get, runs, 1).await;
    // The stop waits for the scheduled dispatch to finish.
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
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
    host.stop_all().await.unwrap();
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
