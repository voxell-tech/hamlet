//! Timers: what a one-shot schedule does when its handler panics,
//! and when its actor sleeps or is destroyed as it comes due, a
//! one-shot that never finishes set aside after five tries, and a
//! stored deadline at either end of the clock's range.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use act::act;
use anyhow::Result;
use async_trait::async_trait;
use hamlet_actor::{
    Action, Actor, ActorConfig, Ctx, Handler, Registry,
};
use hamlet_actor_core::actor::schedule::end_one_shot;
use hamlet_actor_core::{
    ActorEvent, ActorFactory, ActorStart, ShutdownKind,
};
use hamlet_host::{Address, Host, HostEvent, Letter};
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
        || counter::factories(counter::Sleep::WhenIdle),
        "counter",
        ("schedule", schedule_args(SOON_MS, "panic")),
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
        ctx.sql().exec(&count_table("runs")).await?;
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
        ctx.sql().execute(&add_one("runs"), None).await?;
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

/// How long `arm` of the kind `inline` waits for the core to queue
/// the one-shots it scheduled. The core queues a due one-shot well
/// within a millisecond.
const QUEUED: Duration = Duration::from_millis(200);

/// The kind `inline`, of the raw event API, which runs each action
/// inside its event loop, so that the events that come meanwhile wait
/// in its channel. It counts the runs of `boom` and of `inc` in
/// tables of those names.
/// - `arm` schedules a one-shot `boom` due at once and waits for the
///   core to queue it, then schedules three one-shots `inc` due at
///   once and waits for them to queue behind it.
/// - `boom` counts its run and panics, in the event loop.
/// - `inc` counts its run and ends its one-shot, in one transaction.
fn inline() -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("inline".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    HashMap::from([(
        "inline".to_owned(),
        ActorFactory::new(config, |start| {
            Box::pin(run_inline(start))
        }),
    )])
}

async fn run_inline(start: ActorStart) -> Result<()> {
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
                panic!("the event loop panics");
            }
            "inc" => {
                let tx = ctx.sql().begin_transaction(None).await?;
                if let Some(fire) = &scheduled_fire {
                    end_one_shot(&tx, fire).await?;
                }
                tx.execute(&add_one("inc"), None).await?;
                tx.commit().await?;
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
/// runs once after the actor starts again: by the timer the crash
/// stop leaves, or at the next boot. A later boot runs none again.
#[tokio::test(flavor = "multi_thread")]
async fn a_panic_in_the_event_loop_keeps_the_one_shots_queued_behind_it()
 {
    let dir = Scratch::new("timer-loop-panic");
    let host = Host::open(&dir, inline()).unwrap();
    let id = host.get_or_start("inline", "a").await.unwrap();
    act(&host, &id, "arm", Vec::new()).await.unwrap();
    sleep(Duration::from_secs(1)).await;
    host.shutdown().await.unwrap();
    drop(host);

    let mut boots = Vec::new();
    for _ in 0..2 {
        let host = Host::open(&dir, inline()).unwrap();
        boots.push(host.boot().await.unwrap());
        sleep(Duration::from_millis(500)).await;
        host.shutdown().await.unwrap();
        drop(host);
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
    assert!(boots[1].is_empty(), "the second boot started it");
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

/// Milliseconds since the Unix epoch, the clock timers use.
fn now_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    i64::try_from(now.as_millis()).unwrap()
}

/// M1. A one-shot whose handler drops its reply, without a panic, is
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
    let letter = events.try_recv().expect("no dead letter");
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
        letter,
        HostEvent::DeadLetter {
            holder: at.clone(),
            letter: Letter::Timer {
                event_id: timer.event_id.clone(),
                action: timer.action.clone(),
                due_at: timer.due_at,
            },
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
