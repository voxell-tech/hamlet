//! Durable sends: delivered once, in order within a pair, answered
//! from the record when repeated, pruned once confirmed, set aside
//! when the receiver has no address, and delivered once to and from
//! a key destroyed and made again. The receive path's rules: R2, a
//! race of deliveries of one send applies it once; R3, a slow handler
//! is applied once and counts nothing; R15, a new life of a key is
//! delivered once.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use act::{DEADLINE, act};
use anyhow::anyhow;
use hamlet_actor_core::{
    ActorConfig, ActorContext, ActorEvent, ActorFactory, ActorStart,
    BindParam, ShutdownKind,
};
use hamlet_host::send::{self, Delivery};
use hamlet_host::{Address, Cause, Host};
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
}

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
    ] {
        kinds.insert(name.to_owned(), receiver(name, arm));
    }
    kinds
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
    let answers = match arm {
        Arm::Once => vec![receive_once(ctx.clone(), envelope).await?],
        Arm::Twice => vec![
            receive_once(ctx.clone(), envelope.clone()).await?,
            receive_once(ctx.clone(), envelope).await?,
        ],
        Arm::Fan => eight(&ctx, &envelope).join_all().await,
        Arm::Gate => {
            let held = ctx.sql().begin_transaction(None).await?;
            let waiting = eight(&ctx, &envelope);
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

/// One receive of `envelope` as the counter applies it.
async fn receive_once(
    ctx: ActorContext,
    envelope: Vec<u8>,
) -> anyhow::Result<Delivery> {
    send::receive(&ctx, &envelope, |tx, message| async move {
        counter::add_received(&tx, &message).await
    })
    .await
}

/// Eight receives of `envelope`, begun at once. Each answer is the
/// receive's, or `Failed` for an error.
fn eight(ctx: &ActorContext, envelope: &[u8]) -> JoinSet<Delivery> {
    let mut receives = JoinSet::new();
    for _ in 0..8 {
        let (ctx, envelope) = (ctx.clone(), envelope.to_vec());
        receives.spawn(async move {
            receive_once(ctx, envelope).await.unwrap_or_else(
                |error| Delivery::Failed {
                    error: format!("{error:#}"),
                },
            )
        });
    }
    receives
}

/// An answer in a word, with the counter's outcome, its `n`.
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
        Delivery::Failed { .. } => "failed".to_owned(),
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
/// and makes no file.
#[tokio::test(flavor = "multi_thread")]
async fn a_send_with_no_address_is_set_aside() {
    let dir = Scratch::new("send-no-address");
    let host = Host::open(&dir, kinds()).unwrap();
    send_n(&host, "a", "counter/nobody", 1).await;
    send_n(&host, "a", "ghost/b", 2).await;
    drained(&host, "a").await;
    host.shutdown().await.unwrap();

    assert_eq!(
        column::<String>(
            &dir,
            "counter/a",
            "SELECT peer_kind || '/' || peer_key || ' ' || reason \
             FROM _hamlet_dead_letters ORDER BY id"
        ),
        ["counter/nobody no_address", "ghost/b no_address"]
    );
    assert!(!dir.join("actors/counter/nobody.sqlite").exists());
    assert!(!dir.join("actors/ghost").exists());
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
