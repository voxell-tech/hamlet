//! A counter actor. Its count is a row of its own SQL table,
//! `counter`. Every action replies with the count as a little-endian
//! `i64`.
//!
//! - `increment` adds one in a transaction. Fired by a one-shot
//!   schedule, the same transaction ends the one-shot.
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
//! - `block` takes a little-endian `u64` of milliseconds and blocks
//!   the actor's event loop itself that long, in synchronous code, as
//!   an actor stuck in a blocking call does, then replies with the
//!   count. A stop of the actor waits for it.
//! - `call` takes an actor's `<kind>/<key>` and calls its `called` as
//!   this actor. It replies with that reply, then the call's time in
//!   microseconds as a little-endian `u64`.
//! - `called` adds one to the calls of its caller, as the host
//!   stamped it, logs who called, and replies with how many calls it
//!   had from that caller.
//!
//! `increment` passes the fault point `tx-open` inside its
//! transaction, and `tx-committed` after it commits and before it
//! replies.
//!
//! Each action but `block` runs on a task of its own, as the
//! authoring layer runs them, so an action that outlives its
//! caller's deadline does not hold up the next one. Unlike the
//! authoring layer, an action is not dropped when its generation
//! starts to stop: a transaction dropped while open would hold the
//! actor's SQL until it times out.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use hamlet_actor_core::actor::schedule::{
    ScheduledFireInfo, end_one_shot,
};
use hamlet_actor_core::{
    ActorConfig, ActorContext, ActorEvent, ActorFactory, ActorStart,
    BindParam, ColumnValue, ConnHandle,
};
use hamlet_host::{Address, Caller, Host};

/// How long a counter's call to another may take.
const CALL_DEADLINE: Duration = Duration::from_secs(60);

/// Whether counters sleep once idle.
#[derive(Clone, Copy)]
#[cfg_attr(test, expect(dead_code, reason = "the tests use one"))]
pub enum Sleep {
    WhenIdle,
    Never,
}

/// The counter kinds `kinds`.
pub fn factories(
    kinds: &[&str],
    sleep: Sleep,
) -> HashMap<String, ActorFactory> {
    kinds
        .iter()
        .map(|kind| {
            let config = ActorConfig {
                name: Some((*kind).to_owned()),
                has_database: true,
                no_sleep: matches!(sleep, Sleep::Never),
                ..ActorConfig::default()
            };
            let factory = ActorFactory::new(config, |start| {
                Box::pin(run(start))
            });
            ((*kind).to_owned(), factory)
        })
        .collect()
}

/// The count an action replied with.
pub fn count(reply: Vec<u8>) -> Result<i64> {
    let bytes = reply
        .try_into()
        .map_err(|_| anyhow!("the counter replied with no count"))?;
    Ok(i64::from_le_bytes(bytes))
}

/// Runs one generation of a counter.
pub async fn run(start: ActorStart) -> Result<()> {
    let ActorStart {
        ctx, mut events, ..
    } = start;
    ctx.sql()
        .exec(
            "CREATE TABLE IF NOT EXISTS counter (id INTEGER PRIMARY \
             KEY, value INTEGER NOT NULL); CREATE TABLE IF NOT \
             EXISTS calls (caller TEXT PRIMARY KEY, n INTEGER NOT \
             NULL)"
                .to_owned(),
        )
        .await?;
    while let Some(event) = events.recv().await {
        match event {
            ActorEvent::Action {
                name,
                args,
                conn,
                scheduled_fire,
                reply,
                ..
            } => {
                if name == "block" {
                    let count = block(&ctx, &args).await;
                    reply.send(
                        count.map(|count| {
                            count.to_le_bytes().to_vec()
                        }),
                    );
                    continue;
                }
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    let replied = action(
                        &ctx,
                        &name,
                        args,
                        conn.as_ref(),
                        scheduled_fire.as_ref(),
                    )
                    .await;
                    if name != "drop" {
                        reply.send(replied);
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
    conn: Option<&ConnHandle>,
    fire: Option<&ScheduledFireInfo>,
) -> Result<Vec<u8>> {
    let count = match name {
        "increment" => add_one(ctx, fire).await,
        "panic" => {
            add_one(ctx, None).await?;
            panic!("the counter's `panic` action panics");
        }
        "drop" => add_one(ctx, None).await,
        "abort" => {
            add_one(ctx, None).await?;
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
        "call" => return call(ctx, &args).await,
        "called" => called(ctx, conn).await,
        other => Err(anyhow!("unknown action `{other}`")),
    }?;
    Ok(count.to_le_bytes().to_vec())
}

/// Calls the `called` action of the actor whose `<kind>/<key>` is
/// `args`, as the actor of `ctx`. Replies with that reply, then the
/// call's time in microseconds.
async fn call(ctx: &ActorContext, args: &[u8]) -> Result<Vec<u8>> {
    let to = std::str::from_utf8(args)?;
    let (kind, key) = to
        .split_once('/')
        .ok_or_else(|| anyhow!("`{to}` is not <kind>/<key>"))?;
    let begun = Instant::now();
    let mut reply = Host::call_from(
        ctx,
        &Address::new(kind, key),
        "called",
        Vec::new(),
        CALL_DEADLINE,
    )
    .await?;
    let micros = u64::try_from(begun.elapsed().as_micros())?;
    reply.extend_from_slice(&micros.to_le_bytes());
    Ok(reply)
}

/// Adds one to the calls of the caller that `conn` carries, logs it,
/// and answers how many calls the actor had from that caller.
async fn called(
    ctx: &ActorContext,
    conn: Option<&ConnHandle>,
) -> Result<i64> {
    let params = conn.map(ConnHandle::params).unwrap_or_default();
    let caller = match ciborium::from_reader(params.as_slice())? {
        Some(Caller::Actor(at)) => format!("{}/{}", at.kind, at.key),
        Some(Caller::Client) => "client".to_owned(),
        None => "none".to_owned(),
    };
    tracing::info!(actor = ctx.actor_id(), caller, "called");
    let result = ctx
        .sql()
        .execute(
            "INSERT INTO calls (caller, n) VALUES (?, 1) ON \
             CONFLICT (caller) DO UPDATE SET n = n + 1 RETURNING n"
                .to_owned(),
            Some(vec![BindParam::Text(caller)]),
        )
        .await?;
    match result.rows.first().and_then(|row| row.first()) {
        Some(ColumnValue::Integer(n)) => Ok(*n),
        other => Err(anyhow!("the count of calls reads {other:?}")),
    }
}

/// Blocks the calling thread, in synchronous code, for the
/// little-endian `u64` of milliseconds `args` starts with, then reads
/// the count.
async fn block(ctx: &ActorContext, args: &[u8]) -> Result<i64> {
    let ms = args
        .first_chunk()
        .ok_or_else(|| anyhow!("block takes a u64"))?;
    std::thread::sleep(Duration::from_millis(u64::from_le_bytes(
        *ms,
    )));
    read_count(ctx).await
}

async fn add_one(
    ctx: &ActorContext,
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
    tx.commit().await?;
    hamlet_host::fault::point("tx-committed");
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
