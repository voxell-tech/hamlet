//! Durable sends: delivered once, in order within a pair, answered
//! from the record when repeated, pruned once confirmed, set aside
//! when the receiver has no address or its handler failed five runs,
//! and delivered once to and from a key destroyed and made again. The
//! receive path's rules: R2, a race of deliveries of one send applies
//! it once and sets it aside once; R3, a slow handler is applied once
//! and counts nothing; R15, a new life of a key is delivered once. A
//! kind of the authoring layer's trait API receives through
//! `send::receiving` (G3).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use act::{DEADLINE, act};
use anyhow::anyhow;
use hamlet_actor_core::{
    ActorConfig, ActorContext, ActorEvent, ActorFactory, ActorStart,
    BindParam, ColumnValue, ShutdownKind, SqliteTransaction,
};
use hamlet_host::send::{self, Delivery, Message};
use hamlet_host::{Address, Cause, Host, HostEvent, Letter, Reason};
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};
use tokio::task::JoinSet;

#[path = "../examples/demo/act.rs"]
mod act;
#[path = "../examples/demo/counter.rs"]
mod counter;

/// How long a test waits for a send to be confirmed. A delivery takes
/// milliseconds; a slow one, a second or two.
const PATIENCE: Duration = Duration::from_secs(20);

/// How a receiver kind of these tests runs the host's delivery. Each
/// applies a send as the counter does, and keeps each answer it got
/// from `receive` as a row of its table `answers`.
#[derive(Clone, Copy)]
enum Arm {
    /// Receives the envelope once.
    Once,
    /// Receives the envelope twice, one after the other.
    Twice,
    /// Receives the envelope eight times at once.
    Fan,
    /// Holds a transaction while eight receives of the envelope
    /// begin, then ends it, so they are let in together.
    Gate,
    /// The handler takes 600 ms; the kind's action timeout is 200 ms.
    Slow,
    /// The handler fails.
    Fail,
    /// Receives the envelope once, with [`Handler::FailFirst`].
    FailFirst,
    /// Receives the envelope eight times at once, with
    /// [`Handler::FailFirst`].
    FanFailFirst,
}

/// How a receiver kind of these tests applies a send.
#[derive(Clone, Copy)]
enum Handler {
    /// As the counter does.
    Counter,
    /// Fails the pair's first send, and applies the rest as the
    /// counter does. A run of the first send that finds a dead letter
    /// in its file fails with [`LATE`].
    FailFirst,
}

/// The error of [`Handler::FailFirst`]'s first send.
const FAILS_FIRST: &str = "this receiver fails the first send";

/// The error of a run of a send that was already set aside.
const LATE: &str = "a run of a send already set aside";

/// The counters, a receiver kind for each [`Arm`], and the kind
/// `flaky`, which receives as `flaky` does.
fn kinds_with(flaky: Arm) -> HashMap<String, ActorFactory> {
    let mut kinds = kinds();
    kinds.insert("flaky".to_owned(), receiver("flaky", flaky));
    kinds
}

/// The counters and a receiver kind for each [`Arm`].
fn kinds() -> HashMap<String, ActorFactory> {
    let mut kinds = counter::factories(counter::Sleep::WhenIdle);
    for (name, arm) in [
        ("twice", Arm::Twice),
        ("fan", Arm::Fan),
        ("gate", Arm::Gate),
        ("slow", Arm::Slow),
        ("fail", Arm::Fail),
        ("failfirst", Arm::FailFirst),
        ("fanfail", Arm::FanFailFirst),
    ] {
        kinds.insert(name.to_owned(), receiver(name, arm));
    }
    kinds.insert("book".to_owned(), book(0));
    kinds.insert("picky".to_owned(), book(2));
    kinds
}

/// The factory the authoring layer's registry makes for `A`.
fn layer_kind<A: hamlet_actor::Actor>(name: &str) -> ActorFactory {
    let mut registry = hamlet_actor::Registry::new();
    registry.register_actor::<A>(name);
    registry
        .into_factories()
        .remove(name)
        .expect("the registry holds the kind it registered")
}

/// A [`Book`] that receives sends through `send::receiving`. Its
/// handler fails a send's runs until `failing_runs` of them have
/// failed, then writes an `entries` row.
fn book(failing_runs: u32) -> ActorFactory {
    send::receiving(layer_kind::<Book>("book"), move |tx, message| {
        enter(tx, message, failing_runs)
    })
}

async fn enter(
    tx: SqliteTransaction,
    message: Message,
    failing_runs: u32,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        message.failed_runs >= failing_runs,
        "run {} of this send fails",
        message.failed_runs + 1
    );
    let n =
        i64::from_le_bytes(message.payload.as_slice().try_into()?);
    tx.execute(
        "INSERT INTO entries (seq, n, failed_runs) VALUES (?, ?, ?)",
        Some(vec![
            BindParam::Integer(i64::try_from(message.seq)?),
            BindParam::Integer(n),
            BindParam::Integer(i64::from(message.failed_runs)),
        ]),
    )
    .await?;
    Ok(n.to_le_bytes().to_vec())
}

/// An actor of the authoring layer's trait API whose state is the
/// table `entries`, one row per send its handler applied.
struct Book;

/// The number of `entries` rows.
#[derive(serde::Serialize, serde::Deserialize)]
struct Entries;

impl hamlet_actor::Action for Entries {
    type Output = i64;
    const NAME: &'static str = "entries";
}

#[async_trait::async_trait]
impl hamlet_actor::Actor for Book {
    type Input = ();
    type Actions = (Entries,);
    type Events = ();
    type ConnParams = ();
    type Action = ();

    async fn create(
        ctx: &hamlet_actor::Ctx<Self>,
    ) -> anyhow::Result<Self> {
        ctx.sql()
            .exec(
                "CREATE TABLE IF NOT EXISTS entries (seq INTEGER \
                 NOT NULL, n INTEGER NOT NULL, failed_runs INTEGER \
                 NOT NULL)"
                    .to_owned(),
            )
            .await?;
        Ok(Self)
    }
}

impl hamlet_actor::Handler<Entries> for Book {
    async fn handle(
        self: Arc<Self>,
        ctx: hamlet_actor::Ctx<Self>,
        _: Entries,
    ) -> anyhow::Result<i64> {
        let read = ctx
            .sql()
            .execute("SELECT COUNT(*) FROM entries".to_owned(), None)
            .await?;
        match read.rows.first().and_then(|row| row.first()) {
            Some(ColumnValue::Integer(entries)) => Ok(*entries),
            _ => Err(anyhow!("no count of entries")),
        }
    }
}

/// A trait-API actor whose `create` fails.
struct Broken;

#[async_trait::async_trait]
impl hamlet_actor::Actor for Broken {
    type Input = ();
    type Actions = ();
    type Events = ();
    type ConnParams = ();
    type Action = ();

    async fn create(
        _: &hamlet_actor::Ctx<Self>,
    ) -> anyhow::Result<Self> {
        Err(anyhow!("this actor cannot be made"))
    }
}

fn receiver(name: &str, arm: Arm) -> ActorFactory {
    let mut config = ActorConfig {
        name: Some(name.to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    if matches!(arm, Arm::Slow) {
        config.action_timeout = Duration::from_millis(200);
    }
    ActorFactory::new(config, move |start: ActorStart| {
        Box::pin(async move {
            let ActorStart {
                ctx, mut events, ..
            } = start;
            counter::make_tables(&ctx).await?;
            ctx.sql()
                .exec(
                    "CREATE TABLE IF NOT EXISTS answers (id INTEGER \
                     PRIMARY KEY, answer TEXT NOT NULL)"
                        .to_owned(),
                )
                .await?;
            while let Some(event) = events.recv().await {
                match event {
                    ActorEvent::Action {
                        name, args, reply, ..
                    } if name == send::RECEIVE => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            reply.send(receive(ctx, arm, args).await);
                        });
                    }
                    other => counter::answer(other),
                }
            }
            Ok(())
        })
    })
}

async fn receive(
    ctx: ActorContext,
    arm: Arm,
    envelope: Vec<u8>,
) -> anyhow::Result<Vec<u8>> {
    use Handler::Counter;
    let answers = match arm {
        Arm::Once => {
            vec![receive_once(ctx.clone(), envelope, Counter).await?]
        }
        Arm::Twice => vec![
            receive_once(ctx.clone(), envelope.clone(), Counter)
                .await?,
            receive_once(ctx.clone(), envelope, Counter).await?,
        ],
        Arm::Fan => eight(&ctx, &envelope, Counter).join_all().await,
        Arm::Gate => {
            let held = ctx.sql().begin_transaction(None).await?;
            let waiting = eight(&ctx, &envelope, Counter);
            tokio::time::sleep(Duration::from_millis(200)).await;
            held.rollback().await?;
            waiting.join_all().await
        }
        Arm::Slow => vec![
            send::receive(
                &ctx,
                &envelope,
                |tx, message| async move {
                    tokio::time::sleep(Duration::from_millis(600))
                        .await;
                    counter::add_received(&tx, &message).await
                },
            )
            .await?,
        ],
        Arm::Fail => vec![
            send::receive(&ctx, &envelope, |_, _| async {
                Err(anyhow!("this receiver fails"))
            })
            .await?,
        ],
        Arm::FailFirst => vec![
            receive_once(ctx.clone(), envelope, Handler::FailFirst)
                .await?,
        ],
        Arm::FanFailFirst => {
            eight(&ctx, &envelope, Handler::FailFirst)
                .join_all()
                .await
        }
    };
    for answer in &answers {
        ctx.sql()
            .execute(
                "INSERT INTO answers (answer) VALUES (?)".to_owned(),
                Some(vec![BindParam::Text(tag(answer))]),
            )
            .await?;
    }
    answers
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no answer"))?
        .into_reply()
}

/// One receive of `envelope`, applied by `handler`.
async fn receive_once(
    ctx: ActorContext,
    envelope: Vec<u8>,
    handler: Handler,
) -> anyhow::Result<Delivery> {
    send::receive(&ctx, &envelope, |tx, message| {
        apply(handler, tx, message)
    })
    .await
}

async fn apply(
    handler: Handler,
    tx: SqliteTransaction,
    message: Message,
) -> anyhow::Result<Vec<u8>> {
    if matches!(handler, Handler::FailFirst) && message.seq == 1 {
        let read = tx
            .execute(
                "SELECT COUNT(*) FROM _hamlet_dead_letters",
                None,
            )
            .await?;
        let set_aside = matches!(
            read.rows.first().and_then(|row| row.first()),
            Some(ColumnValue::Integer(letters)) if *letters > 0
        );
        anyhow::bail!(if set_aside { LATE } else { FAILS_FIRST });
    }
    counter::add_received(&tx, &message).await
}

/// Eight receives of `envelope`, begun at once, applied by
/// `handler`. Each answer is the receive's, or `Failed` for an error.
fn eight(
    ctx: &ActorContext,
    envelope: &[u8],
    handler: Handler,
) -> JoinSet<Delivery> {
    let mut receives = JoinSet::new();
    for _ in 0..8 {
        let (ctx, envelope) = (ctx.clone(), envelope.to_vec());
        receives.spawn(async move {
            receive_once(ctx, envelope, handler).await.unwrap_or_else(
                |error| Delivery::Failed {
                    error: format!("{error:#}"),
                },
            )
        });
    }
    receives
}

/// An answer in a word, with the counter's outcome, its `n`, a
/// failure's error, or a dead letter's failed runs.
fn tag(answer: &Delivery) -> String {
    let n = |outcome: &[u8]| {
        outcome.try_into().map_or_else(
            |_| "?".to_owned(),
            |n| i64::from_le_bytes(n).to_string(),
        )
    };
    match answer {
        Delivery::Handled { outcome } => {
            format!("handled {}", n(outcome))
        }
        Delivery::Repeat {
            outcome: Some(outcome),
        } => format!("repeat {}", n(outcome)),
        Delivery::Repeat { outcome: None } => "repeat".to_owned(),
        Delivery::OutOfOrder { expected } => {
            format!("out of order {expected}")
        }
        Delivery::Failed { error } => format!("failed: {error}"),
        Delivery::DeadLettered { failed_runs, .. } => {
            format!("dead {failed_runs}")
        }
    }
}

/// Counter `from` sends `n` to the actor `to`, `<kind>/<key>`.
async fn send_n(host: &Host, from: &str, to: &str, n: i64) {
    let mut args = n.to_le_bytes().to_vec();
    args.extend_from_slice(to.as_bytes());
    act(host, &format!("counter/{from}"), "send", args)
        .await
        .unwrap();
}

/// Waits until every send of counter `from` is confirmed.
async fn drained(host: &Host, from: &str) {
    let actor_id = format!("counter/{from}");
    let begun = tokio::time::Instant::now();
    loop {
        let reply =
            act(host, &actor_id, "outbox", Vec::new()).await.unwrap();
        if counter::count(reply).unwrap() == 0 {
            return;
        }
        assert!(
            begun.elapsed() < PATIENCE,
            "{actor_id} still has sends after {PATIENCE:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The first column of each row `sql` reads from the file of
/// `actor_id`, once its host has closed it.
fn column<T: rusqlite::types::FromSql>(
    dir: &Path,
    actor_id: &str,
    sql: &str,
) -> Vec<T> {
    let path = dir.join("actors").join(format!("{actor_id}.sqlite"));
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut statement = conn.prepare(sql).unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn one(dir: &Path, actor_id: &str, sql: &str) -> i64 {
    column(dir, actor_id, sql)[0]
}

/// `(seq, n, failed_runs)` of each `received` row, in the order they
/// were written.
fn received(dir: &Path, actor_id: &str) -> Vec<(i64, i64, i64)> {
    column::<String>(
        dir,
        actor_id,
        "SELECT seq || ' ' || n || ' ' || failed_runs FROM received \
         ORDER BY id",
    )
    .iter()
    .map(|row| {
        let parts: Vec<i64> = row
            .split(' ')
            .map(|part| part.parse().unwrap())
            .collect();
        (parts[0], parts[1], parts[2])
    })
    .collect()
}

/// The answers a receiver of an [`Arm`] got, sorted.
fn answers(dir: &Path, actor_id: &str) -> Vec<String> {
    let mut answers: Vec<String> =
        column(dir, actor_id, "SELECT answer FROM answers");
    answers.sort();
    answers
}

/// A send to a counter that already runs, and whose start found
/// nothing to deliver, is delivered by its commit: once, with its
/// ledger row on the sender and its received row on the receiver.
#[tokio::test(flavor = "multi_thread")]
async fn a_send_is_delivered_once_after_its_commit() {
    let dir = Scratch::new("send-commit");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("counter", "b").await.unwrap();
    drained(&host, "a").await;
    send_n(&host, "a", "counter/b", 5).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    assert_eq!(received(&dir, "counter/b"), vec![(1, 5, 0)]);
    assert_eq!(
        one(&dir, "counter/b", "SELECT value FROM counter"),
        5
    );
    assert_eq!(
        one(&dir, "counter/a", "SELECT COUNT(*) FROM ledger"),
        1
    );
    assert_eq!(
        one(&dir, "counter/a", "SELECT COUNT(*) FROM _hamlet_outbox"),
        0
    );
}

/// A send received again is answered from the record of its first
/// receive, and applied once.
#[tokio::test(flavor = "multi_thread")]
async fn a_repeat_is_answered_from_the_record() {
    let dir = Scratch::new("send-repeat");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("twice", "b").await.unwrap();
    send_n(&host, "a", "twice/b", 7).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    assert_eq!(answers(&dir, "twice/b"), ["handled 7", "repeat 7"]);
    assert_eq!(received(&dir, "twice/b"), vec![(1, 7, 0)]);
}

/// The sends of one pair are applied in the order they were made.
#[tokio::test(flavor = "multi_thread")]
async fn the_sends_of_a_pair_are_applied_in_order() {
    let dir = Scratch::new("send-order");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("counter", "b").await.unwrap();
    for n in 1..=20 {
        send_n(&host, "a", "counter/b", n).await;
    }
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    let expected: Vec<_> = (1..=20).map(|n| (n, n, 0)).collect();
    assert_eq!(received(&dir, "counter/b"), expected);
}

/// The receiver keeps the record of a send only until its sender has
/// confirmed it: each delivery carries the highest send confirmed.
#[tokio::test(flavor = "multi_thread")]
async fn a_record_is_pruned_once_its_sender_confirmed_it() {
    let dir = Scratch::new("send-prune");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("counter", "b").await.unwrap();
    for n in 1..=5 {
        send_n(&host, "a", "counter/b", n).await;
        drained(&host, "a").await;
    }
    host.shutdown().await.unwrap();

    assert_eq!(
        column::<i64>(
            &dir,
            "counter/b",
            "SELECT seq FROM _hamlet_handled"
        ),
        [5]
    );
    assert_eq!(
        one(&dir, "counter/a", "SELECT acked_out FROM _hamlet_peers"),
        5
    );
}

/// R2: eight receives of one send at once apply it once, answer one
/// `Handled` and seven `Repeat`, and leave the pair's next send with
/// no failed run.
#[tokio::test(flavor = "multi_thread")]
async fn eight_receives_of_one_send_apply_it_once() {
    let dir = Scratch::new("send-race");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("fan", "b").await.unwrap();
    send_n(&host, "a", "fan/b", 3).await;
    drained(&host, "a").await;
    send_n(&host, "a", "fan/b", 4).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    let mut expected = vec!["handled 3", "handled 4"];
    expected.extend(["repeat 3"; 7]);
    expected.extend(["repeat 4"; 7]);
    expected.sort_unstable();
    assert_eq!(answers(&dir, "fan/b"), expected);
    assert_eq!(received(&dir, "fan/b"), vec![(1, 3, 0), (2, 4, 0)]);
}

/// R3: a handler slower than its kind's action timeout times its
/// delivery out again and again, and is applied once, its failed runs
/// at 0, and never set aside.
#[tokio::test(flavor = "multi_thread")]
async fn a_slow_handler_is_applied_once() {
    let dir = Scratch::new("send-slow");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("slow", "b").await.unwrap();
    send_n(&host, "a", "slow/b", 1).await;
    send_n(&host, "a", "slow/b", 2).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    assert_eq!(received(&dir, "slow/b"), vec![(1, 1, 0), (2, 2, 0)]);
    assert_eq!(
        one(
            &dir,
            "slow/b",
            "SELECT attempts_in FROM _hamlet_inbound"
        ),
        0
    );
    assert_eq!(
        one(
            &dir,
            "slow/b",
            "SELECT COUNT(*) FROM _hamlet_dead_letters"
        ),
        0
    );
}

/// R3: receives of one send that wait behind a transaction of the
/// receiver and are let in together count nothing: the run that
/// applies it reads 0 failed runs, and so does the pair after it.
#[tokio::test(flavor = "multi_thread")]
async fn receives_let_in_together_count_nothing() {
    let dir = Scratch::new("send-gate");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("gate", "b").await.unwrap();
    send_n(&host, "a", "gate/b", 6).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    let mut expected = vec!["handled 6"];
    expected.extend(["repeat 6"; 7]);
    assert_eq!(answers(&dir, "gate/b"), expected);
    assert_eq!(received(&dir, "gate/b"), vec![(1, 6, 0)]);
    assert_eq!(
        one(
            &dir,
            "gate/b",
            "SELECT attempts_in FROM _hamlet_inbound"
        ),
        0
    );
}

/// A failed run is counted once per run, and the send stays in its
/// sender's outbox to be tried again.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_run_is_counted_and_tried_again() {
    let dir = Scratch::new("send-fail");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("fail", "b").await.unwrap();
    send_n(&host, "a", "fail/b", 1).await;
    // The retries come at 100, 200 and 400 ms.
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    host.shutdown().await.unwrap();

    let failed = column::<String>(
        &dir,
        "fail/b",
        "SELECT answer FROM answers",
    );
    let attempts = one(
        &dir,
        "fail/b",
        "SELECT attempts_in FROM _hamlet_inbound",
    );
    assert!(failed.len() >= 3, "{failed:?}");
    assert_eq!(usize::try_from(attempts).unwrap(), failed.len());
    assert_eq!(received(&dir, "fail/b"), vec![]);
    assert_eq!(
        one(&dir, "counter/a", "SELECT COUNT(*) FROM _hamlet_outbox"),
        1
    );
}

/// A send to a key with no file, or to a kind the host does not hold,
/// is set aside in its sender's file with the reason `no_address`,
/// makes no file, and raises an event with the sender as the holder.
#[tokio::test(flavor = "multi_thread")]
async fn a_send_with_no_address_is_set_aside() {
    let dir = Scratch::new("send-no-address");
    let host = Host::open(&dir, kinds()).unwrap();
    let mut events = host.events();
    send_n(&host, "a", "counter/nobody", 1).await;
    send_n(&host, "a", "ghost/b", 2).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    assert_eq!(
        dead_letters(&dir, "counter/a"),
        [
            "counter/nobody - 1 add no_address 0 -",
            "ghost/b - 1 add no_address 0 -"
        ]
    );
    let no_address = |kind: &str, key: &str| HostEvent::DeadLetter {
        holder: Address::new("counter", "a"),
        letter: Letter::Send {
            peer: Address::new(kind, key),
            peer_file: None,
            seq: 1,
            name: "add".to_owned(),
            reason: Reason::NoAddress,
        },
        tries: 0,
        error: None,
    };
    assert_eq!(
        raised(&mut events),
        [no_address("counter", "nobody"), no_address("ghost", "b")]
    );
    assert!(!dir.join("actors/counter/nobody.sqlite").exists());
    assert!(!dir.join("actors/ghost").exists());
}

/// The id of the file of `actor_id`: which life of its key it is.
fn file_id(dir: &Path, actor_id: &str) -> String {
    column::<String>(
        dir,
        actor_id,
        "SELECT file_id FROM _hamlet_file",
    )
    .remove(0)
}

/// Each `_hamlet_dead_letters` row of `actor_id`, in a line: the
/// peer, its life or `-`, the number, the name, the reason, the
/// failed runs and the last error or `-`.
fn dead_letters(dir: &Path, actor_id: &str) -> Vec<String> {
    column(
        dir,
        actor_id,
        "SELECT peer_kind || '/' || peer_key || ' ' || \
         COALESCE(peer_file, '-') || ' ' || seq || ' ' || name || ' \
         ' || reason || ' ' || attempts || ' ' || COALESCE(error, \
         '-') FROM _hamlet_dead_letters ORDER BY id",
    )
}

/// The events the host raised that `events` has not read yet.
fn raised(
    events: &mut tokio::sync::broadcast::Receiver<HostEvent>,
) -> Vec<HostEvent> {
    std::iter::from_fn(|| events.try_recv().ok()).collect()
}

/// The event of counter `a`'s first send to `failfirst/b`, set aside
/// by its receiver after five failed runs. `a_file` is `a`'s life.
fn first_send_set_aside(a_file: &str) -> HostEvent {
    HostEvent::DeadLetter {
        holder: Address::new("failfirst", "b"),
        letter: Letter::Send {
            peer: Address::new("counter", "a"),
            peer_file: Some(a_file.to_owned()),
            seq: 1,
            name: "add".to_owned(),
            reason: Reason::Attempts,
        },
        tries: 5,
        error: Some(FAILS_FIRST.to_owned()),
    }
}

/// A send whose handler failed five runs is set aside by its
/// receiver instead of run a sixth time: one dead-letter row with the
/// runs and the last error, one event with the receiver as the
/// holder, and the sender's outbox row gone. The pair's next send is
/// applied after it.
#[tokio::test(flavor = "multi_thread")]
async fn a_send_that_fails_five_runs_is_set_aside_and_the_next_applied()
 {
    let dir = Scratch::new("send-dead");
    let host = Host::open(&dir, kinds()).unwrap();
    let mut events = host.events();
    host.get_or_start("failfirst", "b").await.unwrap();
    send_n(&host, "a", "failfirst/b", 1).await;
    send_n(&host, "a", "failfirst/b", 2).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    let a_file = file_id(&dir, "counter/a");
    assert_eq!(raised(&mut events), [first_send_set_aside(&a_file)]);
    assert_eq!(
        dead_letters(&dir, "failfirst/b"),
        [format!(
            "counter/a {a_file} 1 add attempts 5 {FAILS_FIRST}"
        )]
    );
    let mut expected =
        vec!["dead 5".to_owned(), "handled 2".to_owned()];
    expected.extend(vec![format!("failed: {FAILS_FIRST}"); 5]);
    expected.sort();
    assert_eq!(answers(&dir, "failfirst/b"), expected);
    assert_eq!(received(&dir, "failfirst/b"), [(2, 2, 0)]);
    assert_eq!(
        one(
            &dir,
            "failfirst/b",
            "SELECT handled_in FROM _hamlet_inbound"
        ),
        2
    );
    assert_eq!(
        one(&dir, "counter/a", "SELECT acked_out FROM _hamlet_peers"),
        2
    );
}

/// A set-aside send delivered again, as a kill between the
/// receiver's answer and the sender's confirmation leaves it, is
/// answered from its dead-letter row: the handler does not run, no
/// second row is written, the sender raises the event again, and its
/// outbox row goes. The sender's file is set back to that state
/// twice, by hand.
#[tokio::test(flavor = "multi_thread")]
async fn a_set_aside_send_delivered_again_is_answered_from_its_row() {
    let dir = Scratch::new("send-dead-again");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("failfirst", "b").await.unwrap();
    send_n(&host, "a", "failfirst/b", 1).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();
    drop(host);
    let a_file = file_id(&dir, "counter/a");

    for again in 1..=2 {
        let sender =
            Connection::open(dir.join("actors/counter/a.sqlite"))
                .unwrap();
        sender
            .execute_batch(
                "INSERT INTO _hamlet_outbox (to_kind, to_key, seq, \
                 name, payload, task, cause, created_at) VALUES \
                 ('failfirst', 'b', 1, 'add', x'0100000000000000', \
                 'demo', NULL, 0); UPDATE _hamlet_peers SET \
                 acked_out = 0",
            )
            .unwrap();
        drop(sender);

        let host = Host::open(&dir, kinds()).unwrap();
        let mut events = host.events();
        drained(&host, "a").await;
        host.shutdown().await.unwrap();
        drop(host);

        assert_eq!(
            raised(&mut events),
            [first_send_set_aside(&a_file)]
        );
        assert_eq!(
            dead_letters(&dir, "failfirst/b"),
            [format!(
                "counter/a {a_file} 1 add attempts 5 {FAILS_FIRST}"
            )]
        );
        let mut expected = vec!["dead 5".to_owned(); 1 + again];
        expected.extend(vec![format!("failed: {FAILS_FIRST}"); 5]);
        expected.sort();
        assert_eq!(answers(&dir, "failfirst/b"), expected);
        // Its row is its only record. No delivery confirmed a send,
        // so no prune could have taken a handled record.
        assert_eq!(
            one(
                &dir,
                "failfirst/b",
                "SELECT COUNT(*) FROM _hamlet_handled"
            ),
            0
        );
        assert_eq!(
            one(
                &dir,
                "counter/a",
                "SELECT COUNT(*) FROM _hamlet_outbox"
            ),
            0
        );
        assert_eq!(
            one(
                &dir,
                "counter/a",
                "SELECT acked_out FROM _hamlet_peers"
            ),
            1
        );
    }
}

/// R2, the dead-letter half: a failing send received eight times at
/// once in each delivery is set aside once, and no receive after the
/// set-aside runs its handler, in that delivery or a later one. The
/// pair's next send is applied. How the receives interleave decides
/// the split of answers and the runs counted, so rows are checked.
#[tokio::test(flavor = "multi_thread")]
async fn receives_of_a_set_aside_send_run_nothing_and_write_nothing()
{
    let dir = Scratch::new("send-dead-fan");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("fanfail", "b").await.unwrap();
    send_n(&host, "a", "fanfail/b", 1).await;
    send_n(&host, "a", "fanfail/b", 2).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    assert_eq!(
        column::<String>(
            &dir,
            "fanfail/b",
            "SELECT seq || ' ' || reason || ' ' || (attempts >= 5) \
             FROM _hamlet_dead_letters"
        ),
        ["1 attempts 1"]
    );
    let late = format!("failed: {LATE}");
    let answers = answers(&dir, "fanfail/b");
    assert!(!answers.contains(&late), "{answers:?}");
    assert!(answers.contains(&"handled 2".to_owned()), "{answers:?}");
    assert_eq!(received(&dir, "fanfail/b"), [(2, 2, 0)]);
    assert_eq!(
        one(
            &dir,
            "fanfail/b",
            "SELECT handled_in FROM _hamlet_inbound"
        ),
        2
    );
    assert_eq!(
        one(&dir, "counter/a", "SELECT COUNT(*) FROM _hamlet_outbox"),
        0
    );
}

/// R15: a sender destroyed and made again numbers its sends from 1
/// again, and its receiver applies each once.
#[tokio::test(flavor = "multi_thread")]
async fn a_sender_made_again_is_delivered_once() {
    let dir = Scratch::new("send-sender-again");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("counter", "b").await.unwrap();
    send_n(&host, "a", "counter/b", 1).await;
    send_n(&host, "a", "counter/b", 2).await;
    drained(&host, "a").await;
    host.stop("counter/a", ShutdownKind::Destroy).await.unwrap();
    send_n(&host, "a", "counter/b", 3).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    assert_eq!(
        received(&dir, "counter/b"),
        vec![(1, 1, 0), (2, 2, 0), (1, 3, 0)]
    );
    assert_eq!(
        one(&dir, "counter/b", "SELECT value FROM counter"),
        6
    );
}

/// R15: a receiver destroyed and made again takes the next send of
/// its sender as the first of the pair, and applies each once.
#[tokio::test(flavor = "multi_thread")]
async fn a_receiver_made_again_is_delivered_once() {
    let dir = Scratch::new("send-receiver-again");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("counter", "b").await.unwrap();
    send_n(&host, "a", "counter/b", 1).await;
    send_n(&host, "a", "counter/b", 2).await;
    drained(&host, "a").await;
    host.stop("counter/b", ShutdownKind::Destroy).await.unwrap();
    host.get_or_start("counter", "b").await.unwrap();
    send_n(&host, "a", "counter/b", 3).await;
    send_n(&host, "a", "counter/b", 4).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    assert_eq!(
        received(&dir, "counter/b"),
        vec![(3, 3, 0), (4, 4, 0)]
    );
    assert_eq!(
        one(&dir, "counter/b", "SELECT value FROM counter"),
        7
    );
}

/// A sender whose sends were not all confirmed when its host shut
/// down wakes at the next boot, from the deadline its stop left, and
/// delivers them.
#[tokio::test(flavor = "multi_thread")]
async fn a_send_left_at_a_shutdown_is_delivered_after_boot() {
    let dir = Scratch::new("send-boot");
    let host = Host::open(&dir, kinds_with(Arm::Fail)).unwrap();
    host.get_or_start("flaky", "b").await.unwrap();
    send_n(&host, "a", "flaky/b", 8).await;
    host.shutdown().await.unwrap();
    drop(host);

    let host = Host::open(&dir, kinds_with(Arm::Once)).unwrap();
    let started = host.boot().await.unwrap();
    assert!(started.contains(&"counter/a".to_owned()), "{started:?}");
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    // The failing receiver may have run it before the shutdown, or
    // not: its failed runs are not this test's.
    let received: Vec<_> = received(&dir, "flaky/b")
        .into_iter()
        .map(|(seq, n, _)| (seq, n))
        .collect();
    assert_eq!(received, [(1, 8)]);
}

/// A sender that stops while its delivery waits for the receiver
/// loses no send and repeats none: its stop leaves a deadline, its
/// timer starts it again, and its start delivers the send, which the
/// receiver applied once.
#[tokio::test(flavor = "multi_thread")]
async fn a_sender_stopped_during_a_delivery_wakes_and_delivers_once()
{
    let dir = Scratch::new("send-stop-mid");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("slow", "b").await.unwrap();
    send_n(&host, "a", "slow/b", 9).await;
    host.stop("counter/a", ShutdownKind::Sleep).await.unwrap();
    let begun = tokio::time::Instant::now();
    while !host.is_running("counter/a") {
        assert!(begun.elapsed() < PATIENCE, "counter/a never woke");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    assert_eq!(received(&dir, "slow/b"), vec![(1, 9, 0)]);
}

/// R5: a call to an action under `hamlet.` is refused before the host
/// looks up its actor, so a forged delivery neither runs nor makes
/// the receiver's file.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_a_hamlet_action_is_refused() {
    let dir = Scratch::new("send-forged");
    let host = Host::open(&dir, kinds()).unwrap();
    for action in [send::RECEIVE, "hamlet.other"] {
        let error = host
            .call(
                &Address::new("counter", "b"),
                action,
                b"forged".to_vec(),
                DEADLINE,
                Cause::root("test"),
            )
            .await
            .expect_err("a call to a `hamlet.` action must fail");
        assert!(
            format!("{error:#}").contains("`hamlet.`"),
            "{error:#}"
        );
    }
    assert!(!dir.join("actors/counter/b.sqlite").exists());
    assert!(!host.is_running("counter/b"));
    host.shutdown().await.unwrap();
}

/// An actor of the authoring layer's trait API whose one action is
/// named `hamlet.receive`, as only the host may name one.
struct Forger;

#[derive(serde::Serialize, serde::Deserialize)]
struct Forge;

impl hamlet_actor::Action for Forge {
    type Output = ();
    const NAME: &'static str = send::RECEIVE;
}

#[async_trait::async_trait]
impl hamlet_actor::Actor for Forger {
    type Input = ();
    type Actions = (Forge,);
    type Events = ();
    type ConnParams = ();
    type Action = ();

    async fn create(
        _: &hamlet_actor::Ctx<Self>,
    ) -> anyhow::Result<Self> {
        Ok(Self)
    }
}

impl hamlet_actor::Handler<Forge> for Forger {
    async fn handle(
        self: Arc<Self>,
        _: hamlet_actor::Ctx<Self>,
        _: Forge,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// R5: the authoring layer refuses, at registration, an actor that
/// names an action under `hamlet.`.
#[test]
fn registering_a_hamlet_action_fails() {
    let registered = std::panic::catch_unwind(|| {
        hamlet_actor::Registry::new()
            .register_actor::<Forger>("forger");
    });
    let panic = registered.expect_err("the registration must fail");
    let message = panic
        .downcast_ref::<String>()
        .expect("the panic carries a message");
    assert!(message.contains("`hamlet.receive`"), "{message}");
}

/// `(seq, n, failed_runs)` of each `entries` row of a [`Book`].
fn entries(dir: &Path, actor_id: &str) -> Vec<(i64, i64, i64)> {
    column::<String>(
        dir,
        actor_id,
        "SELECT seq || ' ' || n || ' ' || failed_runs FROM entries \
         ORDER BY rowid",
    )
    .iter()
    .map(|row| {
        let parts: Vec<i64> = row
            .split(' ')
            .map(|part| part.parse().unwrap())
            .collect();
        (parts[0], parts[1], parts[2])
    })
    .collect()
}

/// G3: a kind of the trait API applies a send once through
/// `send::receiving`, and its own actions still reach the layer.
#[tokio::test(flavor = "multi_thread")]
async fn a_trait_actor_applies_a_send_once() {
    let dir = Scratch::new("send-trait");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("book", "b").await.unwrap();
    send_n(&host, "a", "book/b", 5).await;
    drained(&host, "a").await;
    let reply = host
        .call(
            &Address::new("book", "b"),
            <Entries as hamlet_actor::Action>::NAME,
            hamlet_actor::action::encode_positional(&Entries)
                .unwrap(),
            DEADLINE,
            Cause::root("test"),
        )
        .await
        .unwrap();
    let count: i64 = ciborium::from_reader(reply.as_slice()).unwrap();
    assert_eq!(count, 1);
    host.shutdown().await.unwrap();

    assert_eq!(entries(&dir, "book/b"), [(1, 5, 0)]);
}

/// G3: a delivery of a send a trait-API kind applied, made again
/// because its sender never confirmed it, is answered without
/// running the handler again. The sender's file is set back to how a
/// kill before the confirmation leaves it.
#[tokio::test(flavor = "multi_thread")]
async fn a_trait_actor_answers_a_repeat_without_applying_it() {
    let dir = Scratch::new("send-trait-repeat");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("book", "b").await.unwrap();
    send_n(&host, "a", "book/b", 5).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();
    drop(host);

    let sender =
        Connection::open(dir.join("actors/counter/a.sqlite"))
            .unwrap();
    sender
        .execute_batch(
            "INSERT INTO _hamlet_outbox (to_kind, to_key, seq, \
             name, payload, task, cause, created_at) VALUES \
             ('book', 'b', 1, 'add', x'0500000000000000', 'demo', \
             NULL, 0); UPDATE _hamlet_peers SET acked_out = 0",
        )
        .unwrap();
    drop(sender);

    let host = Host::open(&dir, kinds()).unwrap();
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    // The send left the outbox again, confirmed rather than set
    // aside, and the handler did not run a second time.
    assert_eq!(
        one(&dir, "counter/a", "SELECT acked_out FROM _hamlet_peers"),
        1
    );
    assert_eq!(entries(&dir, "book/b"), [(1, 5, 0)]);
    assert_eq!(
        one(&dir, "book/b", "SELECT COUNT(*) FROM _hamlet_handled"),
        1
    );
    assert_eq!(
        one(
            &dir,
            "counter/a",
            "SELECT COUNT(*) FROM _hamlet_dead_letters"
        ),
        0
    );
}

/// G3: a trait-API kind's handler that fails is counted and tried
/// again; the run that applies sees the runs that failed.
#[tokio::test(flavor = "multi_thread")]
async fn a_trait_actors_failed_runs_are_counted_and_tried_again() {
    let dir = Scratch::new("send-trait-fail");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start("picky", "b").await.unwrap();
    send_n(&host, "a", "picky/b", 5).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    assert_eq!(entries(&dir, "picky/b"), [(1, 5, 2)]);
}

/// G3: the layer of a wrapped kind that fails to start ends the
/// wrapper's run too, so the start fails rather than hang.
#[tokio::test(flavor = "multi_thread")]
async fn a_trait_actor_whose_create_fails_fails_its_start() {
    let dir = Scratch::new("send-trait-broken");
    let host = Host::open(
        &dir,
        HashMap::from([(
            "broken".to_owned(),
            send::receiving(
                layer_kind::<Broken>("broken"),
                |_, _| async { Ok(Vec::new()) },
            ),
        )]),
    )
    .unwrap();
    let started = tokio::time::timeout(
        PATIENCE,
        host.get_or_start("broken", "b"),
    )
    .await
    .expect("a failed create ends the start");
    assert!(started.is_err());
    host.shutdown().await.unwrap();
}
