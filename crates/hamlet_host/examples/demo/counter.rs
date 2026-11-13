//! A counter actor. Its count is a row of its own SQL table,
//! `counter`. Every action replies with the count as a little-endian
//! `i64`.
//!
//! - `increment` adds one in a transaction. Fired by a one-shot
//!   schedule, the same transaction ends the one-shot.
//! - `discard` adds one in a transaction and rolls it back.
//! - `get` reads the count.
//! - `panic` adds one in a transaction that does not end its
//!   one-shot, then panics.
//! - `drop` adds one in a transaction that does not end its one-shot,
//!   then drops its reply unsent.
//! - `abort` adds one in a transaction that does not end its
//!   one-shot, then aborts the process.
//! - `schedule` takes a little-endian `u64` of milliseconds, then
//!   optionally an action's name, and arms a one-shot of that action,
//!   `increment` without one, that far ahead. It replies with the
//!   count it read before, so its reply waits for no write after the
//!   schedule's.
//! - `sleep` and `destroy` ask the host to sleep or destroy the
//!   actor, and reply with the count.
//! - `send` takes a little-endian `i64` `n`, then the receiver's id,
//!   `<kind>/<key>`. In one transaction it writes a `ledger` row and
//!   sends `n` to the receiver, which adds it to its count.
//! - `poison` takes the receiver's id and sends it, as `send` does
//!   with `n` 0, a message whose handler aborts the process.
//! - `outbox` replies with the number of this counter's sends that
//!   their receivers have not confirmed.
//! - `hamlet.receive` is the host's delivery of a send. Its receive
//!   transaction adds the send's `n` to the count and writes a
//!   `received` row, or aborts the process for a `poison` message. A
//!   delivery the host answers as a repeat writes a `repeats` row
//!   instead. It replies with the host's answer, not a count.
//!
//! `increment` passes the fault point `tx-open` inside its
//! transaction, and `tx-committed` after it commits and before it
//! replies. `send` passes `send-tx-open` inside its transaction,
//! after the send, and a receive passes `receive-tx-open` inside its
//! receive transaction.
//!
//! Each action runs on a task of its own, as the authoring layer
//! runs them, so an action that outlives its caller's deadline does
//! not hold up the next one. Unlike the authoring layer, an action is
//! not dropped when its generation starts to stop: a transaction
//! dropped while open would hold the actor's SQL until it times out.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use hamlet_actor_core::actor::schedule::{
    ScheduledFireInfo, end_one_shot,
};
use hamlet_actor_core::{
    ActorConfig, ActorContext, ActorEvent, ActorFactory, ActorStart,
    BindParam, ColumnValue, SqliteTransaction,
};
use hamlet_host::send::{self, Delivery, Message};
use hamlet_host::{Address, Cause};

/// Whether counters sleep once idle.
#[derive(Clone, Copy)]
#[cfg_attr(test, expect(dead_code, reason = "the tests use one"))]
pub enum Sleep {
    WhenIdle,
    Never,
}

/// The actor kinds the demo host runs.
pub fn factories(sleep: Sleep) -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("counter".to_owned()),
        has_database: true,
        no_sleep: matches!(sleep, Sleep::Never),
        ..ActorConfig::default()
    };
    HashMap::from([(
        "counter".to_owned(),
        ActorFactory::new(config, |start| Box::pin(run(start))),
    )])
}

/// The count an action replied with.
pub fn count(reply: Vec<u8>) -> Result<i64> {
    let bytes = reply
        .try_into()
        .map_err(|_| anyhow!("the counter replied with no count"))?;
    Ok(i64::from_le_bytes(bytes))
}

/// Makes the counter's tables: its count, the sends it made, the
/// sends it received and the deliveries it was answered a repeat for.
pub async fn make_tables(ctx: &ActorContext) -> Result<()> {
    ctx.sql()
        .exec(
            "CREATE TABLE IF NOT EXISTS counter (id INTEGER PRIMARY \
             KEY, value INTEGER NOT NULL); CREATE TABLE IF NOT \
             EXISTS ledger (id INTEGER PRIMARY KEY, to_id TEXT NOT \
             NULL, seq INTEGER NOT NULL, n INTEGER NOT NULL); \
             CREATE TABLE IF NOT EXISTS received (id INTEGER \
             PRIMARY KEY, from_id TEXT NOT NULL, seq INTEGER NOT \
             NULL, n INTEGER NOT NULL, failed_runs INTEGER NOT \
             NULL); CREATE TABLE IF NOT EXISTS repeats (id INTEGER \
             PRIMARY KEY)"
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Runs one generation of a counter.
pub async fn run(start: ActorStart) -> Result<()> {
    let ActorStart {
        ctx, mut events, ..
    } = start;
    make_tables(&ctx).await?;
    while let Some(event) = events.recv().await {
        match event {
            ActorEvent::Action {
                name,
                args,
                scheduled_fire,
                reply,
                ..
            } if name == send::RECEIVE => {
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    reply.send(
                        receive(&ctx, &args, scheduled_fire.as_ref())
                            .await,
                    );
                });
            }
            ActorEvent::Action {
                name,
                args,
                scheduled_fire,
                reply,
                ..
            } => {
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    let count = action(
                        &ctx,
                        &name,
                        args,
                        scheduled_fire.as_ref(),
                    )
                    .await;
                    if name != "drop" {
                        reply.send(count.map(|count| {
                            count.to_le_bytes().to_vec()
                        }));
                    }
                });
            }
            other => answer(other),
        }
    }
    // The loop ends when the core closes the channel, after the
    // stop's last event.
    Ok(())
}

/// Answers the lifecycle events of an actor whose state is all in
/// SQL: it has no other state to save, nothing to clean up, and lets
/// every connection in. Drops any other event's reply.
pub fn answer(event: ActorEvent) {
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

async fn action(
    ctx: &ActorContext,
    name: &str,
    args: Vec<u8>,
    fire: Option<&ScheduledFireInfo>,
) -> Result<i64> {
    match name {
        "increment" => add_one(ctx, true, fire).await,
        "discard" => add_one(ctx, false, fire).await,
        "panic" => {
            add_one(ctx, true, None).await?;
            panic!("the counter's `panic` action panics");
        }
        "drop" => add_one(ctx, true, None).await,
        "abort" => {
            add_one(ctx, true, None).await?;
            std::process::abort();
        }
        "get" => read_count(ctx).await,
        "sleep" => {
            ctx.sleep()?;
            read_count(ctx).await
        }
        "destroy" => {
            ctx.destroy()?;
            read_count(ctx).await
        }
        "schedule" => {
            let (ms, action) = args
                .split_first_chunk()
                .ok_or_else(|| anyhow!("schedule takes a u64"))?;
            let action = match std::str::from_utf8(action)? {
                "" => "increment",
                action => action,
            };
            let count = read_count(ctx).await?;
            ctx.after(
                Duration::from_millis(u64::from_le_bytes(*ms)),
                action,
                &[],
            )
            .await?;
            Ok(count)
        }
        "send" => {
            let (n, to) = args
                .split_first_chunk()
                .ok_or_else(|| anyhow!("send takes an i64"))?;
            send_n(
                ctx,
                "add",
                i64::from_le_bytes(*n),
                std::str::from_utf8(to)?,
            )
            .await
        }
        "poison" => {
            send_n(ctx, "poison", 0, std::str::from_utf8(&args)?)
                .await
        }
        "outbox" => {
            let result = ctx
                .sql()
                .execute(
                    "SELECT COUNT(*) FROM _hamlet_outbox".to_owned(),
                    None,
                )
                .await?;
            Ok(
                match result.rows.first().and_then(|row| row.first())
                {
                    Some(ColumnValue::Integer(pending)) => *pending,
                    _ => 0,
                },
            )
        }
        other => Err(anyhow!("unknown action `{other}`")),
    }
}

/// Sends the message `name` with `n` to the actor `to`,
/// `<kind>/<key>`, with a `ledger` row in the same transaction.
async fn send_n(
    ctx: &ActorContext,
    name: &str,
    n: i64,
    to: &str,
) -> Result<i64> {
    let (kind, key) = to
        .split_once('/')
        .with_context(|| format!("`{to}` is not <kind>/<key>"))?;
    let tx = ctx.sql().begin_transaction(None).await?;
    let seq = send::send(
        &tx,
        &Address::new(kind, key),
        name,
        &n.to_le_bytes(),
        &Cause::root("demo"),
    )
    .await?;
    tx.execute(
        "INSERT INTO ledger (to_id, seq, n) VALUES (?, ?, ?)",
        Some(vec![
            BindParam::Text(to.to_owned()),
            BindParam::Integer(i64::try_from(seq)?),
            BindParam::Integer(n),
        ]),
    )
    .await?;
    hamlet_host::fault::point("send-tx-open");
    tx.commit().await?;
    read_count(ctx).await
}

/// The counter's `hamlet.receive`: receives the send in `envelope`
/// and replies with the host's answer, after a `repeats` row when
/// the answer is a repeat. A one-shot of this counter's own cannot
/// be a delivery, so one that names this action is refused.
async fn receive(
    ctx: &ActorContext,
    envelope: &[u8],
    fire: Option<&ScheduledFireInfo>,
) -> Result<Vec<u8>> {
    if fire.is_some() {
        bail!(
            "`{}` is the host's delivery, not a timer",
            send::RECEIVE
        );
    }
    let delivery =
        send::receive(ctx, envelope, |tx, message| async move {
            if message.name == "poison" {
                std::process::abort();
            }
            add_received(&tx, &message).await
        })
        .await?;
    if matches!(delivery, Delivery::Repeat { .. }) {
        ctx.sql()
            .execute(
                "INSERT INTO repeats DEFAULT VALUES".to_owned(),
                None,
            )
            .await?;
    }
    delivery.into_reply()
}

/// Adds the received send's `n` to the count and writes its
/// `received` row, in its receive transaction. Its outcome is `n`.
pub async fn add_received(
    tx: &SqliteTransaction,
    message: &Message,
) -> Result<Vec<u8>> {
    let n = i64::from_le_bytes(
        message
            .payload
            .as_slice()
            .try_into()
            .context("a send to a counter carries an i64")?,
    );
    tx.execute(
        "INSERT INTO counter (id, value) VALUES (0, ?1) ON CONFLICT \
         (id) DO UPDATE SET value = value + ?1",
        Some(vec![BindParam::Integer(n)]),
    )
    .await?;
    tx.execute(
        "INSERT INTO received (from_id, seq, n, failed_runs) VALUES \
         (?, ?, ?, ?)",
        Some(vec![
            BindParam::Text(format!(
                "{}/{}",
                message.from.kind, message.from.key
            )),
            BindParam::Integer(i64::try_from(message.seq)?),
            BindParam::Integer(n),
            BindParam::Integer(i64::from(message.failed_runs)),
        ]),
    )
    .await?;
    hamlet_host::fault::point("receive-tx-open");
    Ok(n.to_le_bytes().to_vec())
}

async fn add_one(
    ctx: &ActorContext,
    commit: bool,
    fire: Option<&ScheduledFireInfo>,
) -> Result<i64> {
    let tx = ctx.sql().begin_transaction(None).await?;
    if let Some(fire) = fire {
        end_one_shot(&tx, fire).await?;
    }
    tx.execute(
        "INSERT INTO counter (id, value) VALUES (0, 1) ON CONFLICT \
         (id) DO UPDATE SET value = value + 1",
        None,
    )
    .await?;
    hamlet_host::fault::point("tx-open");
    if commit {
        tx.commit().await?;
        hamlet_host::fault::point("tx-committed");
    } else {
        tx.rollback().await?;
    }
    read_count(ctx).await
}

async fn read_count(ctx: &ActorContext) -> Result<i64> {
    let result = ctx
        .sql()
        .execute(
            "SELECT value FROM counter WHERE id = ?".to_owned(),
            Some(vec![BindParam::Integer(0)]),
        )
        .await?;
    Ok(match result.rows.first().and_then(|row| row.first()) {
        Some(ColumnValue::Integer(value)) => *value,
        _ => 0,
    })
}
