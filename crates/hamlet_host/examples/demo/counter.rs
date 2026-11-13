//! A counter actor. Its count is a row of its own SQL table,
//! `counter`. Every action replies with the count as a little-endian
//! `i64`.
//!
//! - `increment` adds one in a transaction. Fired by a one-shot
//!   schedule, the same transaction ends the one-shot.
//! - `discard` adds one in a transaction and rolls it back.
//! - `get` reads the count.
//! - `schedule` takes a little-endian `u64` of milliseconds and arms
//!   a one-shot `increment` that far ahead. It replies with the count
//!   it read before, so its reply waits for no write after the
//!   schedule's.
//! - `sleep` and `destroy` ask the host to sleep or destroy the
//!   actor, and reply with the count.
//!
//! `increment` passes the fault point `tx-open` inside its
//! transaction, and `tx-committed` after it commits and before it
//! replies.
//!
//! Each action runs on a task of its own, as the authoring layer
//! runs them, so an action that outlives its caller's deadline does
//! not hold up the next one. Unlike the authoring layer, an action is
//! not dropped when its generation starts to stop: a transaction
//! dropped while open would hold the actor's SQL until it times out.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Result, anyhow};
use hamlet_actor_core::actor::schedule::{
    ScheduledFireInfo, end_one_shot,
};
use hamlet_actor_core::{
    ActorConfig, ActorContext, ActorEvent, ActorFactory, ActorStart,
    BindParam, ColumnValue,
};

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
                    reply.send(
                        count.map(|count| {
                            count.to_le_bytes().to_vec()
                        }),
                    );
                });
            }
            // The count is in SQL, so the actor keeps no other state.
            ActorEvent::SerializeState { reply, .. } => {
                reply.send(Ok(Vec::new()));
            }
            // The loop ends when the core closes the channel, after
            // the stop's last event.
            ActorEvent::RunGracefulCleanup { reply, .. }
            | ActorEvent::ConnectionPreflight { reply, .. }
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
    fire: Option<&ScheduledFireInfo>,
) -> Result<i64> {
    match name {
        "increment" => add_one(ctx, true, fire).await,
        "discard" => add_one(ctx, false, fire).await,
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
