//! Timers: what a one-shot schedule does when its handler panics,
//! and when its actor sleeps or is destroyed as it comes due, a
//! one-shot that never finishes set aside after five tries, a
//! stored deadline at either end of the clock's range, a start for a
//! deadline that fails, a double drain, twenty one-shots due at once,
//! and a handler that returns an error, its own or a call's.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use act::act;
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use clock::now_ms;
use code::code;
use hamlet_actor::{
    Action, Actor, ActorConfig, Ctx, Handler, Registry,
};
use hamlet_actor_core::actor::schedule::{
    ScheduledFireInfo, end_one_shot,
};
use hamlet_actor_core::sqlite::SqliteDb;
use hamlet_actor_core::{
    ActorContext, ActorEvent, ActorFactory, ActorStart, BindParam,
    ColumnValue, ShutdownKind,
};
use hamlet_host::{Address, Caller, Host, HostEvent, HostOptions};
use hamlet_testing::Scratch;
use serde::{Deserialize, Serialize};
use tokio::time::{Instant, sleep};

#[path = "../examples/demo/act.rs"]
mod act;
#[path = "support/clock.rs"]
mod clock;
#[path = "support/code.rs"]
mod code;
#[path = "../examples/demo/counter.rs"]
mod counter;

fn factories() -> HashMap<String, ActorFactory> {
    counter::factories(&["counter"], counter::Sleep::WhenIdle)
}

/// How far ahead a test schedules its one-shot.
const SOON_MS: u64 = 10;

/// What one actor's file holds, read with its file closed.
#[derive(Debug, PartialEq)]
struct Stored {
    runs: i64,
    schedules: i64,
}

/// Opens the actor's file to read. No host may hold it: a host
/// holds its actors' files alone.
fn closed_file(dir: &Path, actor_id: &str) -> rusqlite::Connection {
    rusqlite::Connection::open_with_flags(
        dir.join(format!("actors/{actor_id}.sqlite")),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}

/// Reads the actor's file. `runs` is the one row of its table
/// `table`.
fn stored(dir: &Path, actor_id: &str, table: &str) -> Stored {
    let conn = closed_file(dir, actor_id);
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

/// The generation the actor's file last started and the deadline its
/// last stop left.
fn last_start(dir: &Path, actor_id: &str) -> (i64, Option<i64>) {
    closed_file(dir, actor_id)
        .query_row(
            "SELECT generation, wake_at FROM _hamlet_actor",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

/// Makes the table `table`, of one count, as [`stored`] reads it.
fn count_table(table: &str) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS {table} (id INTEGER PRIMARY \
         KEY, value INTEGER NOT NULL)"
    )
}

/// Adds one to the count in the table `table`.
fn add_one(table: &str) -> String {
    format!(
        "INSERT INTO {table} (id, value) VALUES (0, 1) ON CONFLICT \
         (id) DO UPDATE SET value = value + 1"
    )
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

/// The demo counter's `schedule` arguments: a one-shot of `action`
/// `ms` ahead.
fn schedule_args(ms: u64, action: &str) -> Vec<u8> {
    let mut args = ms.to_le_bytes().to_vec();
    args.extend_from_slice(action.as_bytes());
    args
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
    a_panicking_handler_runs_once(
        &dir,
        factories,
        "counter",
        ("schedule", schedule_args(SOON_MS, "panic")),
        ("get", |reply| counter::count(reply).unwrap()),
        "counter",
    )
    .await;
}

/// An actor of the authoring layer's trait API. `arm` schedules a
/// one-shot `fire`, which records its run in the table `runs`; `runs`
/// reads the record. Then `fire` panics, or, if `NAPS`, waits for
/// good on its first run and ends on any later one.
///
/// `fire`'s record lands before its panic, and a stop between the two
/// cuts it short: the authoring layer answers it as cut off, which
/// keeps its one-shot to run again. So `fire` holds `firing`
/// until it ends or naps, and `runs` takes it first: a run it counts
/// has ended, or naps.
struct Fuse<const NAPS: bool> {
    firing: tokio::sync::Mutex<()>,
}

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
impl<const NAPS: bool> Actor for Fuse<NAPS> {
    type Input = ();
    type Actions = (Arm, Fire, Runs);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        ctx.sql().exec(&count_table("runs")).await?;
        Ok(Self {
            firing: tokio::sync::Mutex::new(()),
        })
    }
}

impl<const NAPS: bool> Handler<Arm> for Fuse<NAPS> {
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

impl<const NAPS: bool> Handler<Fire> for Fuse<NAPS> {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Fire,
    ) -> Result<()> {
        let firing = self.firing.lock().await;
        ctx.sql().execute(&add_one("runs"), None).await?;
        if !NAPS {
            panic!("`fire` panics");
        }
        if count_in(ctx.sql(), "runs").await? == 1 {
            drop(firing);
            std::future::pending::<()>().await;
        }
        Ok(())
    }
}

impl<const NAPS: bool> Handler<Runs> for Fuse<NAPS> {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Runs,
    ) -> Result<i64> {
        let _firing = self.firing.lock().await;
        count_in(ctx.sql(), "runs").await
    }
}

/// The kind `kind`, a [`Fuse`].
fn fuse_factories<const NAPS: bool>(
    kind: &str,
) -> HashMap<String, ActorFactory> {
    let mut registry = Registry::new();
    registry.register_actor_with::<Fuse<NAPS>>(
        kind,
        ActorConfig {
            name: Some(kind.to_owned()),
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry.into_factories()
}

/// Reads `runs`' reply.
fn fuse_runs(reply: Vec<u8>) -> i64 {
    ciborium::from_reader(&reply[..]).unwrap()
}

/// The trait API: `fire` panics in the task the authoring layer runs
/// it on, which holds its reply.
#[tokio::test(flavor = "multi_thread")]
async fn a_one_shot_whose_trait_handler_panics_runs_once() {
    let dir = Scratch::new("timer-panic-trait");
    a_panicking_handler_runs_once(
        &dir,
        || fuse_factories::<false>("fuse"),
        "fuse",
        ("arm", Vec::new()),
        ("runs", fuse_runs),
        "runs",
    )
    .await;
}

/// A stop cuts short a one-shot's handler of the trait API that
/// recorded its run outside the one-shot's transaction: the handler
/// is answered as cut off, `actor.stopping`, so its one-shot is kept,
/// and the deadline the stop stores starts the actor again at once,
/// which runs it again. Such a handler runs at least once; one that
/// ends its one-shot in its own transaction runs once.
#[tokio::test(flavor = "multi_thread")]
async fn a_one_shot_whose_trait_handler_a_stop_cuts_short_runs_again()
{
    let dir = Scratch::new("timer-cut-trait");
    let factories = || fuse_factories::<true>("nap");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("nap", "a").await.unwrap();
    act(&host, &id, "arm", Vec::new()).await.unwrap();
    wait_for_runs(&host, &id, "runs", fuse_runs, 1).await;
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    // Its own timer starts it: a call would start it too.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !host.is_running(&id) {
        assert!(
            Instant::now() < deadline,
            "not woken after its stop"
        );
        sleep(Duration::from_millis(10)).await;
    }
    wait_for_runs(&host, &id, "runs", fuse_runs, 2).await;
    host.shutdown().await.unwrap();
    drop(host);
    assert_eq!(
        stored(&dir, &id, "runs"),
        Stored {
            runs: 2,
            schedules: 0
        }
    );

    let host = Host::open(&dir, factories()).unwrap();
    assert!(host.boot().await.unwrap().is_empty());
    wait_for_runs(&host, &id, "runs", fuse_runs, 2).await;
    host.shutdown().await.unwrap();
}

/// The `actor` codes a call to another actor fails with when the
/// callee did not run the action to its end: it was starting,
/// stopping or gone, too busy, or out of time.
const CALL_FAILURES: [&str; 10] = [
    "starting",
    "not_ready",
    "stopping",
    "destroying",
    "destroyed",
    "shutdown_timeout",
    "dropped_reply",
    "overloaded",
    "aborted",
    "action_timed_out",
];

/// An actor of the trait API whose one-shot `pass` records its run in
/// the table `runs`, then returns the error of a call it made to
/// another actor. Its key names that error's `actor` code. For
/// `action_timed_out`, `pass` makes the call: to `nap` of
/// `relay/callee`, which never ends, with 50 ms to run. For any other
/// code it returns the error such a call fails with. `arm` schedules
/// `pass`; `runs` reads the record; `pending` counts the one-shots
/// fired whose handler has not answered and been finished.
struct Relay;

#[derive(Serialize, Deserialize)]
struct Pass;

#[derive(Serialize, Deserialize)]
struct Nap;

#[derive(Serialize, Deserialize)]
struct Pending;

impl Action for Pass {
    type Output = ();
    const NAME: &'static str = "pass";
}

impl Action for Nap {
    type Output = ();
    const NAME: &'static str = "nap";
}

impl Action for Pending {
    type Output = i64;
    const NAME: &'static str = "pending";
}

#[async_trait]
impl Actor for Relay {
    type Input = ();
    type Actions = (Arm, Pass, Nap, Runs, Pending);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        ctx.sql().exec(&count_table("runs")).await?;
        Ok(Self)
    }
}

impl Handler<Arm> for Relay {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Arm,
    ) -> Result<()> {
        ctx.schedule()
            .after(Duration::from_millis(SOON_MS), Pass::NAME, &[])
            .await?;
        Ok(())
    }
}

impl Handler<Pass> for Relay {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Pass,
    ) -> Result<()> {
        ctx.sql().execute(&add_one("runs"), None).await?;
        let (_, code) =
            ctx.actor_id().split_once('/').context("an actor id")?;
        if code == "action_timed_out" {
            Host::call_from(
                ctx.inner(),
                &Address::new("relay", "callee"),
                Nap::NAME,
                Vec::new(),
                Duration::from_millis(50),
            )
            .await?;
            anyhow::bail!("a call to `nap` ended");
        }
        Err(hamlet_actor_error::ActorError {
            kind: hamlet_actor_error::ActorErrorKind::Dynamic {
                group: "actor".to_owned(),
                code: code.to_owned(),
                default_message: format!("the call failed: {code}"),
            },
            meta: None,
            message: None,
            actor: None,
        }
        .into())
    }
}

impl Handler<Nap> for Relay {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Nap,
    ) -> Result<()> {
        std::future::pending().await
    }
}

impl Handler<Runs> for Relay {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Runs,
    ) -> Result<i64> {
        count_in(ctx.sql(), "runs").await
    }
}

impl Handler<Pending> for Relay {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Pending,
    ) -> Result<i64> {
        let read = ctx
            .sql()
            .query(
                "SELECT COUNT(*) FROM _actor_schedule_events WHERE \
                 claimed_generation IS NOT NULL AND last_error IS \
                 NULL",
                None,
            )
            .await?;
        match read.rows.first().and_then(|row| row.first()) {
            Some(ColumnValue::Integer(pending)) => Ok(*pending),
            other => anyhow::bail!("pending read {other:?}"),
        }
    }
}

/// A one-shot whose handler returns the error of a call it made runs
/// once, whatever that error's code: the handler ran to its end. Its
/// row goes with the run, so its next generation does not run it
/// again, and nothing is set aside.
#[tokio::test(flavor = "multi_thread")]
async fn a_one_shot_whose_handler_returns_a_calls_error_runs_once() {
    let dir = Scratch::new("timer-call-error");
    let mut registry = Registry::new();
    registry.register_actor_with::<Relay>(
        "relay",
        ActorConfig {
            name: Some("relay".to_owned()),
            has_database: true,
            ..ActorConfig::default()
        },
    );
    let host = Host::open(&dir, registry.into_factories()).unwrap();
    host.get_or_start("relay", "callee").await.unwrap();
    let mut ids = Vec::new();
    for code in CALL_FAILURES {
        let id = host.get_or_start("relay", code).await.unwrap();
        act(&host, &id, "arm", Vec::new()).await.unwrap();
        ids.push(id);
    }
    for id in &ids {
        wait_for_runs(&host, id, "runs", fuse_runs, 1).await;
        wait_for_runs(&host, id, "pending", fuse_runs, 0).await;
    }
    // A kept one-shot runs again when its actor's next generation
    // starts.
    for id in &ids {
        host.stop(id, ShutdownKind::Sleep).await.unwrap();
    }
    for code in CALL_FAILURES {
        host.get_or_start("relay", code).await.unwrap();
    }
    sleep(Duration::from_millis(500)).await;
    let mut ran = Vec::new();
    for (code, id) in CALL_FAILURES.iter().zip(&ids) {
        let runs = fuse_runs(
            act(&host, id, "runs", Vec::new()).await.unwrap(),
        );
        let set_aside = host
            .set_aside_timers(&Address::new("relay", *code))
            .await
            .unwrap();
        ran.push((*code, runs, set_aside.len()));
    }
    host.shutdown().await.unwrap();
    drop(host);
    let stored_one_shots = ids
        .iter()
        .map(|id| stored(&dir, id, "runs").schedules)
        .collect::<Vec<_>>();
    assert_eq!(
        ran,
        CALL_FAILURES.map(|code| (code, 1, 0)),
        "(code, runs, set aside)"
    );
    assert_eq!(stored_one_shots, [0; 10], "one-shots still stored");
}

/// How long `arm` of the kind `inline` waits for the core to queue
/// the one-shots it scheduled. The core queues a due one-shot well
/// within a millisecond.
const QUEUED: Duration = Duration::from_millis(200);

/// The runs of `boom` and of `inc` that a kind [`inline`] finished:
/// `boom`'s once its count is written, `inc`'s once committed.
#[derive(Default)]
struct Ran {
    boom: AtomicU32,
    inc: AtomicU32,
}

/// The kind `inline`, of the raw event API, which runs each action
/// inside its event loop, so that the events that come meanwhile wait
/// in its channel. It counts the runs of `boom` and of `inc` in
/// tables of those names, and in `ran`.
/// - `arm` schedules a one-shot `boom` due at once and waits for the
///   core to queue it, then schedules three one-shots `inc` due at
///   once and waits for them to queue behind it.
/// - `boom` counts its run and panics, in the event loop.
/// - `inc` counts its run and ends its one-shot, in one transaction.
fn inline(ran: &Arc<Ran>) -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("inline".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    HashMap::from([(
        "inline".to_owned(),
        ActorFactory::new(config, {
            let ran = ran.clone();
            move |start| Box::pin(run_inline(start, ran.clone()))
        }),
    )])
}

async fn run_inline(start: ActorStart, ran: Arc<Ran>) -> Result<()> {
    let ActorStart {
        ctx, mut events, ..
    } = start;
    for table in ["boom", "inc"] {
        ctx.sql().exec(count_table(table)).await?;
    }
    while let Some(event) = events.recv().await {
        let ActorEvent::Action {
            name,
            scheduled_fire,
            reply,
            ..
        } = event
        else {
            counter::answer(event);
            continue;
        };
        match name.as_str() {
            "arm" => {
                ctx.after(Duration::ZERO, "boom", &[]).await?;
                sleep(QUEUED).await;
                for _ in 0..3 {
                    ctx.after(Duration::ZERO, "inc", &[]).await?;
                }
                sleep(QUEUED).await;
            }
            "boom" => {
                ctx.sql().execute(add_one("boom"), None).await?;
                ran.boom.fetch_add(1, Ordering::SeqCst);
                panic!("the event loop panics");
            }
            "inc" => {
                let tx = ctx.sql().begin_transaction(None).await?;
                if let Some(fire) = &scheduled_fire {
                    end_one_shot(&tx, fire).await?;
                }
                tx.execute(&add_one("inc"), None).await?;
                tx.commit().await?;
                ran.inc.fetch_add(1, Ordering::SeqCst);
            }
            _ => {}
        }
        reply.send(Ok(Vec::new()));
    }
    Ok(())
}

/// The event loop of a raw actor panics in one one-shot's handler
/// while three other one-shots wait in its channel. The panic ends
/// only the row of the one-shot whose handler it cut short. The three
/// never reached the actor's code, so their rows are kept, and each
/// runs once when the timer the crash stop leaves starts the actor
/// again. A later boot starts it for none of them.
#[tokio::test(flavor = "multi_thread")]
async fn a_panic_in_the_event_loop_keeps_the_one_shots_queued_behind_it()
 {
    let dir = Scratch::new("timer-loop-panic");
    let ran = Arc::new(Ran::default());
    let host = Host::open(&dir, inline(&ran)).unwrap();
    let id = host.get_or_start("inline", "a").await.unwrap();
    act(&host, &id, "arm", Vec::new()).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let runs = || {
        (
            ran.boom.load(Ordering::SeqCst),
            ran.inc.load(Ordering::SeqCst),
        )
    };
    while runs().0 < 1 || runs().1 < 3 {
        assert!(Instant::now() < deadline, "ran {:?}", runs());
        sleep(Duration::from_millis(10)).await;
    }
    host.shutdown().await.unwrap();
    drop(host);

    for _ in 0..2 {
        let host = Host::open(&dir, inline(&ran)).unwrap();
        let booted = host.boot().await.unwrap();
        host.shutdown().await.unwrap();
        drop(host);
        assert!(booted.is_empty(), "a boot started {booted:?}");
        assert_eq!(
            stored(&dir, &id, "inc"),
            Stored {
                runs: 3,
                schedules: 0
            },
            "the queued one-shots did not each run once"
        );
        assert_eq!(
            stored(&dir, &id, "boom").runs,
            1,
            "the panicking handler did not run once"
        );
    }
    assert_eq!(runs(), (1, 3));
}

/// How many generations of a kind have begun.
type Starts = Arc<AtomicU32>;

/// The demo counter, sleeping after 200 ms idle. Counts its
/// generations in `starts`.
fn counters_that_sleep_soon(
    starts: &Starts,
) -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("counter".to_owned()),
        has_database: true,
        sleep_timeout: Duration::from_millis(200),
        ..ActorConfig::default()
    };
    let starts = starts.clone();
    HashMap::from([(
        "counter".to_owned(),
        ActorFactory::new(config, move |start| {
            starts.fetch_add(1, Ordering::SeqCst);
            Box::pin(counter::run(start))
        }),
    )])
}

/// Waits until `starts` reaches `want`, then for `still`, and returns
/// the count then.
async fn starts_after(
    starts: &Starts,
    want: u32,
    still: Duration,
) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(20);
    while starts.load(Ordering::SeqCst) < want {
        assert!(
            Instant::now() < deadline,
            "{} starts, not {want}",
            starts.load(Ordering::SeqCst)
        );
        sleep(Duration::from_millis(10)).await;
    }
    sleep(still).await;
    starts.load(Ordering::SeqCst)
}

/// A one-shot whose handler drops its reply, without a panic, is
/// kept to be tried again. Each generation tries it once, and its
/// idle stop leaves the past deadline that starts the next at once.
/// The sixth finds it tried five times, sets it aside, and stops for
/// good. A re-arm tries it again.
#[tokio::test(flavor = "multi_thread")]
async fn a_one_shot_that_drops_its_reply_is_set_aside_after_five_runs()
 {
    let dir = Scratch::new("timer-drop-loop");
    let starts = Starts::default();
    let host =
        Host::open(&dir, counters_that_sleep_soon(&starts)).unwrap();
    let mut events = host.events();
    let id = host.get_or_start("counter", "a").await.unwrap();
    act(&host, &id, "schedule", schedule_args(SOON_MS, "drop"))
        .await
        .unwrap();
    assert_eq!(
        starts_after(&starts, 6, Duration::from_secs(2)).await,
        6,
        "it started again after the sixth generation"
    );
    assert!(!host.is_running(&id));
    let set_aside = events.try_recv().expect("no set-aside event");
    assert!(events.try_recv().is_err(), "a second event");
    host.shutdown().await.unwrap();
    drop(host);
    assert_eq!(
        stored(&dir, &id, "counter"),
        Stored {
            runs: 5,
            schedules: 0
        }
    );
    assert_eq!(last_start(&dir, &id), (6, None));

    let host =
        Host::open(&dir, counters_that_sleep_soon(&starts)).unwrap();
    let at = Address::new("counter", "a");
    let timers = host.set_aside_timers(&at).await.unwrap();
    let [timer] = &timers[..] else {
        panic!("set aside: {timers:?}");
    };
    assert_eq!(
        (timer.action.as_str(), timer.tries),
        ("drop", 5),
        "{timer:?}"
    );
    assert_eq!(
        set_aside,
        HostEvent::TimerSetAside {
            actor: at.clone(),
            event_id: timer.event_id.clone(),
            action: timer.action.clone(),
            due_at: timer.due_at,
            tries: 5,
            error: Some("actor.dropped_reply".to_owned()),
        }
    );
    assert_eq!(
        timer.last_error.as_deref(),
        Some("actor.dropped_reply")
    );
    assert!(
        host.rearm_timer(&at, &timer.event_id, now_ms())
            .await
            .unwrap()
    );
    wait_for_runs(
        &host,
        &id,
        "get",
        |reply| counter::count(reply).unwrap(),
        6,
    )
    .await;
    host.shutdown().await.unwrap();
}

/// How far ahead `arm` of the kind `poison` schedules its one-shot.
const POISON_DUE: Duration = Duration::from_millis(200);

/// The kind `poison`, of the raw event API. Every action replies with
/// the count of `inc`'s runs, as the demo counter does. Counts its
/// generations in `starts`.
/// - `arm` schedules a one-shot `inc` `POISON_DUE` ahead and poisons
///   the actor: from its next start, its event loop panics 20 ms
///   after the start, before it reads an event.
/// - `inc` counts its run.
fn poisoned(starts: &Starts) -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("poison".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    let starts = starts.clone();
    HashMap::from([(
        "poison".to_owned(),
        ActorFactory::new(config, move |start| {
            starts.fetch_add(1, Ordering::SeqCst);
            Box::pin(run_poisoned(start))
        }),
    )])
}

async fn run_poisoned(start: ActorStart) -> Result<()> {
    let ActorStart {
        ctx, mut events, ..
    } = start;
    for table in ["poison", "inc"] {
        ctx.sql().exec(count_table(table)).await?;
    }
    if count_in(ctx.sql(), "poison").await? > 0 {
        sleep(Duration::from_millis(20)).await;
        panic!("the event loop panics before its first event");
    }
    while let Some(event) = events.recv().await {
        let ActorEvent::Action { name, reply, .. } = event else {
            counter::answer(event);
            continue;
        };
        match name.as_str() {
            "arm" => {
                ctx.after(POISON_DUE, "inc", &[]).await?;
                ctx.sql().execute(add_one("poison"), None).await?;
            }
            "inc" => {
                ctx.sql().execute(add_one("inc"), None).await?;
            }
            _ => {}
        }
        reply.send(
            count_in(ctx.sql(), "inc")
                .await
                .map(|count| count.to_le_bytes().to_vec()),
        );
    }
    Ok(())
}

/// The count in the table `table`, as [`count_table`] makes it.
async fn count_in(sql: &SqliteDb, table: &str) -> Result<i64> {
    let result = sql
        .query(
            format!("SELECT COALESCE(MAX(value), 0) FROM {table}"),
            None,
        )
        .await?;
    match result.rows.first().and_then(|row| row.first()) {
        Some(ColumnValue::Integer(count)) => Ok(*count),
        other => anyhow::bail!("{table} read {other:?}"),
    }
}

/// From its second generation the actor's event loop
/// panics before it reads the one-shot queued for it, so the one-shot
/// is kept, and each crash stop leaves the past deadline that starts
/// the next generation at once. Generations 2 to 6 each try it once.
/// The seventh sets it aside, and its crash stop leaves no deadline.
/// Cured, the actor runs the one-shot once it is re-armed.
#[tokio::test(flavor = "multi_thread")]
async fn a_loop_that_panics_before_its_one_shot_sets_it_aside_after_five_starts()
 {
    let dir = Scratch::new("timer-poison-loop");
    let starts = Starts::default();
    let host = Host::open(&dir, poisoned(&starts)).unwrap();
    let mut events = host.events();
    let id = host.get_or_start("poison", "a").await.unwrap();
    act(&host, &id, "arm", Vec::new()).await.unwrap();
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    assert_eq!(
        starts_after(&starts, 7, Duration::from_secs(1)).await,
        7,
        "it started again after the seventh generation"
    );
    assert!(!host.is_running(&id));
    let set_aside = events.try_recv().expect("no set-aside event");
    assert!(events.try_recv().is_err(), "a second event");
    host.shutdown().await.unwrap();
    drop(host);
    assert_eq!(
        stored(&dir, &id, "inc"),
        Stored {
            runs: 0,
            schedules: 0
        }
    );
    assert_eq!(last_start(&dir, &id), (7, None));

    rusqlite::Connection::open(
        dir.join(format!("actors/{id}.sqlite")),
    )
    .unwrap()
    .execute("DELETE FROM poison", [])
    .unwrap();
    let host = Host::open(&dir, poisoned(&starts)).unwrap();
    let at = Address::new("poison", "a");
    let timers = host.set_aside_timers(&at).await.unwrap();
    let [timer] = &timers[..] else {
        panic!("set aside: {timers:?}");
    };
    assert_eq!(
        (timer.action.as_str(), timer.tries),
        ("inc", 5),
        "{timer:?}"
    );
    assert_eq!(
        set_aside,
        HostEvent::TimerSetAside {
            actor: at.clone(),
            event_id: timer.event_id.clone(),
            action: timer.action.clone(),
            due_at: timer.due_at,
            tries: 5,
            error: timer.last_error.clone(),
        }
    );
    assert!(
        host.rearm_timer(&at, &timer.event_id, now_ms())
            .await
            .unwrap()
    );
    wait_for_runs(
        &host,
        &id,
        "get",
        |reply| counter::count(reply).unwrap(),
        1,
    )
    .await;
    host.shutdown().await.unwrap();
    drop(host);
    assert_eq!(
        stored(&dir, &id, "inc"),
        Stored {
            runs: 1,
            schedules: 0
        }
    );
}

/// The due time of each one-shot in a sweep.
const SWEEP_DUE_MS: u64 = 100;

/// Starts one counter per step, `<prefix><n>`. Each schedules a
/// one-shot `increment` and is stopped for `reason` at its own moment
/// near the one-shot's due time, in steps of 100 us from 3 ms before
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

/// Each counter sleeps near the time its one-shot comes due,
/// before, while or after it runs. However the stop and the one-shot
/// fall, the one-shot runs once: a stop that comes first leaves a
/// deadline that wakes the actor for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sleep_near_the_due_time_keeps_the_timer() {
    let dir = Scratch::new("timer-sleep-sweep");
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

/// Each counter is destroyed near the time its one-shot comes
/// due. No timer brings a destroyed actor back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_alarm_due_during_a_destroy_does_not_revive_it() {
    let dir = Scratch::new("timer-destroy-sweep");
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
/// never makes a file. Its start finds no actor and is not tried
/// again, and the key is then free for a create.
#[tokio::test(flavor = "multi_thread")]
async fn a_timer_does_not_make_a_file_that_is_gone() {
    let dir = Scratch::new("timer-file-gone");
    let host = Host::open_with(
        &dir,
        factories(),
        HostOptions {
            start_retry: Duration::from_millis(100),
            ..HostOptions::default()
        },
    )
    .unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    act(&host, &id, "schedule", 300u64.to_le_bytes().to_vec())
        .await
        .unwrap();
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    let file = dir.join(format!("actors/{id}.sqlite"));
    std::fs::remove_file(&file).unwrap();
    // The key holds an actor asleep until its timer's start finds
    // none.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(!file.exists(), "the timer made the file again");
        let Err(error) =
            host.create("counter", "a", Vec::new()).await
        else {
            break;
        };
        assert_eq!(code(&error), "actor.duplicate_key", "{error:#}");
        assert!(Instant::now() < deadline, "the key stayed taken");
        sleep(Duration::from_millis(20)).await;
    }
    let reply = act(&host, &id, "get", Vec::new()).await.unwrap();
    assert_eq!(counter::count(reply).unwrap(), 0);
    host.shutdown().await.unwrap();
}

/// Stores `wake_at` as the deadline of a stopped counter, as its last
/// stop would, and boots a host on the folder. Returns the host, the
/// counter's id and the starts boot began. Fails if boot does not
/// return within five seconds.
async fn boot_with_deadline(
    dir: &Path,
    wake_at: i64,
) -> (Host, String, Vec<String>) {
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
            .expect("boot does not return on a due deadline")
            .unwrap();
    (host, id, started)
}

/// A deadline at the lowest time is past, so boot begins its actor's
/// start at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_deadline_at_the_lowest_time_starts_the_actor_at_once() {
    let dir = Scratch::new("timer-deadline-min");
    let (host, id, started) =
        boot_with_deadline(&dir, i64::MIN).await;
    assert_eq!(started, vec![id.clone()]);
    // Boot does not wait for the start it began.
    until_running(&host, &id).await;
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

/// The demo counter, whose start fails while `failures` is above
/// zero, taking one from it each time. Counts its generations in
/// `starts`.
fn counters_that_fail_to_start(
    starts: &Starts,
    failures: &Starts,
) -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("counter".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    let (starts, failures) = (starts.clone(), failures.clone());
    HashMap::from([(
        "counter".to_owned(),
        ActorFactory::new_with_manual_startup_ready(
            config,
            move |mut start: ActorStart| {
                starts.fetch_add(1, Ordering::SeqCst);
                let fails = failures
                    .try_update(
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                        |n| n.checked_sub(1),
                    )
                    .is_ok();
                let ready = start.startup_ready.take();
                Box::pin(async move {
                    // Its ready signal dropped, the start fails.
                    anyhow::ensure!(!fails, "the start fails");
                    if let Some(ready) = ready {
                        let _ = ready.send(Ok(()));
                    }
                    counter::run(start).await
                })
            },
        ),
    )])
}

/// A host of the counters of [`counters_that_fail_to_start`], whose
/// start retry is `retry`.
fn open_failing(
    dir: &Path,
    starts: &Starts,
    failures: &Starts,
    retry: Duration,
) -> Host {
    Host::open_with(
        dir,
        counters_that_fail_to_start(starts, failures),
        HostOptions {
            start_retry: retry,
            ..HostOptions::default()
        },
    )
    .unwrap()
}

/// Waits until the actor `id` runs.
async fn until_running(host: &Host, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !host.is_running(id) {
        assert!(Instant::now() < deadline, "{id} never started");
        sleep(Duration::from_millis(10)).await;
    }
}

/// A start that a timer began for its actor's deadline, and that
/// failed, does not lose the deadline: after the host's start retry,
/// a timer starts the actor again, as many times as it takes. The
/// one-shot that was due runs then.
#[tokio::test(flavor = "multi_thread")]
async fn a_timer_whose_start_failed_is_armed_again() {
    let dir = Scratch::new("timer-start-retry");
    let starts = Starts::default();
    let failures = Starts::default();
    let host = open_failing(
        &dir,
        &starts,
        &failures,
        Duration::from_millis(300),
    );
    let id = host.get_or_start("counter", "a").await.unwrap();
    act(&host, &id, "schedule", schedule_args(300, "increment"))
        .await
        .unwrap();
    failures.store(2, Ordering::SeqCst);
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();

    assert_eq!(starts_after(&starts, 4, Duration::ZERO).await, 4);
    until_running(&host, &id).await;
    let reply = act(&host, &id, "get", Vec::new()).await.unwrap();
    assert_eq!(counter::count(reply).unwrap(), 1);
    assert_eq!(starts.load(Ordering::SeqCst), 4);
    host.shutdown().await.unwrap();
}

/// Boot's start of an actor that a kill left running, which fails,
/// is tried again after the host's start retry, as many times as it
/// takes.
#[tokio::test(flavor = "multi_thread")]
async fn boot_tries_a_run_a_kill_cut_off_again_until_it_starts() {
    let dir = Scratch::new("timer-boot-retry");
    let starts = Starts::default();
    let failures = Starts::default();
    let retry = Duration::from_millis(300);
    let host = open_failing(&dir, &starts, &failures, retry);
    let id = host.get_or_start("counter", "a").await.unwrap();
    host.shutdown().await.unwrap();
    drop(host);
    // What a kill leaves of a running actor.
    rusqlite::Connection::open(
        dir.join(format!("actors/{id}.sqlite")),
    )
    .unwrap()
    .execute("UPDATE _hamlet_actor SET running = 1", [])
    .unwrap();
    failures.store(2, Ordering::SeqCst);

    let host = open_failing(&dir, &starts, &failures, retry);
    assert_eq!(host.boot().await.unwrap(), std::slice::from_ref(&id));
    assert_eq!(starts_after(&starts, 4, Duration::ZERO).await, 4);
    until_running(&host, &id).await;
    host.shutdown().await.unwrap();
}

/// A call's start that fails is not tried again, and keeps the
/// deadline its actor held, whether a stop left it or a failed
/// start's retry did: the timer still starts the actor then, and the
/// one-shot that was due runs.
#[tokio::test(flavor = "multi_thread")]
async fn a_calls_failed_start_keeps_the_deadline_its_actor_held() {
    let dir = Scratch::new("timer-call-keeps");
    let starts = Starts::default();
    let failures = Starts::default();
    let host = open_failing(
        &dir,
        &starts,
        &failures,
        Duration::from_secs(1),
    );
    let id = host.get_or_start("counter", "a").await.unwrap();
    act(&host, &id, "schedule", schedule_args(1500, "increment"))
        .await
        .unwrap();
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    failures.store(3, Ordering::SeqCst);

    // Asleep until its one-shot is due, the actor is started by the
    // call itself.
    let error = act(&host, &id, "get", Vec::new()).await.unwrap_err();
    assert_eq!(starts.load(Ordering::SeqCst), 2, "{error:#}");
    // The timer's start at the deadline fails too.
    assert_eq!(starts_after(&starts, 3, Duration::ZERO).await, 3);
    // A create of the key starts nothing, and fails with
    // `actor.duplicate_key` once that start's failure left the actor
    // asleep with its retry; while the start runs, a create waits
    // for it and fails with its error.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let error = host
            .create("counter", "a", Vec::new())
            .await
            .unwrap_err();
        if code(&error) == "actor.duplicate_key" {
            break;
        }
        assert!(Instant::now() < deadline, "the start never ended");
        sleep(Duration::from_millis(10)).await;
    }
    let error = act(&host, &id, "get", Vec::new()).await.unwrap_err();
    assert_eq!(starts.load(Ordering::SeqCst), 4, "{error:#}");

    // The retry's start.
    assert_eq!(starts_after(&starts, 5, Duration::ZERO).await, 5);
    until_running(&host, &id).await;
    wait_for_runs(
        &host,
        &id,
        "get",
        |reply| counter::count(reply).unwrap(),
        1,
    )
    .await;
    assert_eq!(starts.load(Ordering::SeqCst), 5);
    host.shutdown().await.unwrap();
}

/// A call's start of an actor that held no deadline, which fails, is
/// not tried again.
#[tokio::test(flavor = "multi_thread")]
async fn a_calls_failed_start_of_an_actor_with_no_deadline_is_not_tried_again()
 {
    let dir = Scratch::new("timer-call-no-retry");
    let starts = Starts::default();
    let failures = Starts::default();
    let host = open_failing(
        &dir,
        &starts,
        &failures,
        Duration::from_millis(100),
    );
    let id = host.get_or_start("counter", "a").await.unwrap();
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    failures.store(1, Ordering::SeqCst);

    act(&host, &id, "get", Vec::new()).await.unwrap_err();
    let after =
        starts_after(&starts, 2, Duration::from_millis(500)).await;
    assert_eq!(after, 2, "the failed start was tried again");
    assert!(!host.is_running(&id));
    host.shutdown().await.unwrap();
}

/// The kind `timed`, of the raw event API, whose one-shot handlers
/// count their runs in the table `runs`, by action and event. Each
/// action runs on a task of its own.
/// - `twice` schedules a one-shot `inc` due at once, then drains the
///   due one-shots twice at once.
/// - `twenty` schedules twenty one-shots `slow`, due 10 ms apart.
/// - `fail` schedules a one-shot `error` due at once.
/// - `inc` counts its run and ends its one-shot, in one transaction.
/// - `slow` waits 50 ms, then does as `inc` does, so the runs of
///   one-shots due close together overlap.
/// - `error` counts its run, then returns an error.
/// - `read` replies with what [`read_runs`] reads of the action its
///   arguments name.
fn timed() -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("timed".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    HashMap::from([(
        "timed".to_owned(),
        ActorFactory::new(config, |start| Box::pin(run_timed(start))),
    )])
}

async fn run_timed(start: ActorStart) -> Result<()> {
    let ActorStart {
        ctx, mut events, ..
    } = start;
    ctx.sql()
        .exec(
            "CREATE TABLE IF NOT EXISTS runs (action TEXT NOT NULL, \
             event TEXT NOT NULL, n INTEGER NOT NULL, PRIMARY KEY \
             (action, event))",
        )
        .await?;
    while let Some(event) = events.recv().await {
        let ActorEvent::Action {
            name,
            args,
            scheduled_fire,
            reply,
            ..
        } = event
        else {
            counter::answer(event);
            continue;
        };
        let ctx = ctx.clone();
        tokio::spawn(async move {
            reply.send(
                timed_action(&ctx, &name, &args, scheduled_fire)
                    .await,
            );
        });
    }
    Ok(())
}

async fn timed_action(
    ctx: &ActorContext,
    name: &str,
    args: &[u8],
    fire: Option<ScheduledFireInfo>,
) -> Result<Vec<u8>> {
    const COUNT: &str = "INSERT INTO runs (action, event, n) VALUES \
                         (?, ?, 1) ON CONFLICT (action, event) DO \
                         UPDATE SET n = n + 1";
    let counted = |fire: &ScheduledFireInfo| {
        Some(vec![
            BindParam::Text(name.to_owned()),
            BindParam::Text(fire.id.clone()),
        ])
    };
    match name {
        "twice" => {
            ctx.after(Duration::ZERO, "inc", &[]).await?;
            tokio::try_join!(
                ctx.drain_overdue_scheduled_events(),
                ctx.drain_overdue_scheduled_events()
            )?;
        }
        "twenty" => {
            for n in 0..20 {
                ctx.after(Duration::from_millis(10 * n), "slow", &[])
                    .await?;
            }
        }
        "fail" => {
            ctx.after(Duration::ZERO, "error", &[]).await?;
        }
        "inc" | "slow" => {
            let fire = fire.context("a one-shot's action")?;
            if name == "slow" {
                sleep(Duration::from_millis(50)).await;
            }
            let tx = ctx.sql().begin_transaction(None).await?;
            end_one_shot(&tx, &fire).await?;
            tx.execute(COUNT, counted(&fire)).await?;
            tx.commit().await?;
        }
        "error" => {
            let fire = fire.context("a one-shot's action")?;
            ctx.sql().execute(COUNT, counted(&fire)).await?;
            anyhow::bail!("the handler returns an error");
        }
        "read" => {
            let action = std::str::from_utf8(args)?.to_owned();
            let runs = ctx
                .sql()
                .execute(
                    "SELECT COALESCE(SUM(n), 0), COUNT(*), \
                     COALESCE(MAX(n), 0), (SELECT COUNT(*) FROM \
                     _actor_schedule_events) FROM runs WHERE action \
                     = ?",
                    Some(vec![BindParam::Text(action)]),
                )
                .await?;
            let row = runs.rows.first().context("no row")?;
            let mut reply = Vec::new();
            for value in row {
                let ColumnValue::Integer(value) = value else {
                    anyhow::bail!("a count reads {value:?}");
                };
                reply.extend_from_slice(&value.to_le_bytes());
            }
            return Ok(reply);
        }
        other => anyhow::bail!("unknown action `{other}`"),
    }
    Ok(Vec::new())
}

/// What the kind `timed` stored of the runs of `action`: how many
/// runs, of how many one-shots, the most runs of one, and how many
/// one-shots are still stored.
#[derive(Debug, PartialEq)]
struct Tally {
    runs: i64,
    one_shots: i64,
    most: i64,
    stored: i64,
}

async fn read_runs(host: &Host, id: &str, action: &str) -> Tally {
    let reply = act(host, id, "read", action.as_bytes().to_vec())
        .await
        .unwrap();
    let values = reply
        .as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| i64::from_le_bytes(*chunk))
        .collect::<Vec<_>>();
    let [runs, one_shots, most, stored] = values[..] else {
        panic!("read replied {values:?}");
    };
    Tally {
        runs,
        one_shots,
        most,
        stored,
    }
}

/// Waits until the runs of `action` read `want`, then for 300 ms, and
/// checks they still read it.
async fn runs_settle(
    host: &Host,
    id: &str,
    action: &str,
    want: Tally,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let read = read_runs(host, id, action).await;
        if read == want {
            break;
        }
        assert!(Instant::now() < deadline, "{action}: {read:?}");
        sleep(Duration::from_millis(10)).await;
    }
    sleep(Duration::from_millis(300)).await;
    assert_eq!(read_runs(host, id, action).await, want, "{action}");
}

/// The runs of `n` one-shots, each run once and none left stored.
fn each_once(n: i64) -> Tally {
    Tally {
        runs: n,
        one_shots: n,
        most: 1,
        stored: 0,
    }
}

/// A one-shot that comes due runs once though its generation drains
/// the due one-shots twice at once, beside the drain of its own
/// alarm: one drain claims it, and the others pass it over.
#[tokio::test(flavor = "multi_thread")]
async fn a_double_drain_runs_a_one_shot_once() {
    let dir = Scratch::new("timer-double-drain");
    let host = Host::open(&dir, timed()).unwrap();
    let id = host.get_or_start("timed", "a").await.unwrap();
    for round in 1..=20 {
        act(&host, &id, "twice", Vec::new()).await.unwrap();
        runs_settle(&host, &id, "inc", each_once(round)).await;
    }
    host.shutdown().await.unwrap();
}

/// Twenty one-shots due 10 ms apart, whose handlers each take 50 ms
/// and so run at once, each run once.
#[tokio::test(flavor = "multi_thread")]
async fn twenty_overlapping_timers_each_run_once() {
    let dir = Scratch::new("timer-twenty");
    let host = Host::open(&dir, timed()).unwrap();
    let id = host.get_or_start("timed", "a").await.unwrap();
    act(&host, &id, "twenty", Vec::new()).await.unwrap();
    runs_settle(&host, &id, "slow", each_once(20)).await;
    host.shutdown().await.unwrap();
}

/// A one-shot whose handler returns an error runs once: its row goes
/// with the run, so neither its generation nor the next tries it
/// again, and nothing is set aside.
#[tokio::test(flavor = "multi_thread")]
async fn a_one_shot_whose_handler_returns_an_error_runs_once() {
    let dir = Scratch::new("timer-error");
    let host = Host::open(&dir, timed()).unwrap();
    let id = host.get_or_start("timed", "a").await.unwrap();
    act(&host, &id, "fail", Vec::new()).await.unwrap();
    runs_settle(&host, &id, "error", each_once(1)).await;
    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    runs_settle(&host, &id, "error", each_once(1)).await;
    let set_aside = host
        .set_aside_timers(&Address::new("timed", "a"))
        .await
        .unwrap();
    assert!(set_aside.is_empty(), "{set_aside:?}");
    host.shutdown().await.unwrap();
}
