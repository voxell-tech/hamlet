//! The counter actor the demo and the host's tests run. Its count is
//! a row of its own SQL table, `counter`. Every action replies with
//! the count as a little-endian `i64`.
//!
//! - `increment` adds one in a transaction.
//! - `discard` adds one in a transaction and rolls it back.
//! - `get` reads the count.
//! - `schedule` takes a little-endian `u64` of milliseconds and arms
//!   a one-shot `increment` that far ahead. It replies with the count
//!   it read before, so its reply waits for no write after the
//!   schedule's.
//!
//! `increment` passes the fault point `tx-open` inside its
//! transaction.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Result, anyhow};
use hamlet_actor_core::{
    ActorConfig, ActorContext, ActorEvent, ActorFactory, ActorStart,
    BindParam, ColumnValue, SerializeStateReason,
};

/// The actor kinds the demo host runs.
pub fn factories() -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("counter".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    HashMap::from([(
        "counter".to_owned(),
        ActorFactory::new(config, |start| Box::pin(run(start))),
    )])
}

async fn run(start: ActorStart) -> Result<()> {
    let ActorStart {
        ctx, mut events, ..
    } = start;
    ctx.sql()
        .exec(
            "CREATE TABLE IF NOT EXISTS counter (id INTEGER PRIMARY \
             KEY, value INTEGER NOT NULL)"
                .to_owned(),
        )
        .await?;
    while let Some(event) = events.recv().await {
        match event {
            ActorEvent::Action {
                name, args, reply, ..
            } => {
                let count = action(&ctx, &name, args).await;
                reply.send(
                    count.map(|count| count.to_le_bytes().to_vec()),
                );
            }
            ActorEvent::SerializeState {
                reason: SerializeStateReason::Save,
                reply,
            } => reply.send(Ok(Vec::new())),
            ActorEvent::RunGracefulCleanup { reply, .. } => {
                reply.send(Ok(()));
                break;
            }
            ActorEvent::ConnectionPreflight { reply, .. }
            | ActorEvent::ConnectionOpen { reply, .. }
            | ActorEvent::DisconnectConn { reply, .. } => {
                reply.send(Ok(()));
            }
            _ => {}
        }
    }
    Ok(())
}

async fn action(
    ctx: &ActorContext,
    name: &str,
    args: Vec<u8>,
) -> Result<i64> {
    match name {
        "increment" => add_one(ctx, true).await,
        "discard" => add_one(ctx, false).await,
        "get" => read_count(ctx).await,
        "schedule" => {
            let ms = u64::from_le_bytes(
                args.try_into()
                    .map_err(|_| anyhow!("schedule takes a u64"))?,
            );
            let count = read_count(ctx).await?;
            ctx.after(Duration::from_millis(ms), "increment", &[])
                .await?;
            Ok(count)
        }
        other => Err(anyhow!("unknown action `{other}`")),
    }
}

async fn add_one(ctx: &ActorContext, commit: bool) -> Result<i64> {
    let tx = ctx.sql().begin_transaction(None).await?;
    tx.execute(
        "INSERT INTO counter (id, value) VALUES (0, 1) ON CONFLICT \
         (id) DO UPDATE SET value = value + 1",
        None,
    )
    .await?;
    hamlet_host::fault::point("tx-open");
    if commit {
        tx.commit().await?;
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
