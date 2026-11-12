//! The local host: start by key, stop, the folder lock, alarms armed
//! at start, and the commit hook.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Result, anyhow};
use hamlet_actor_core::{
    ActorConfig, ActorEvent, ActorFactory, ActorStart, BindParam,
    ColumnValue, SerializeStateReason, ShutdownKind,
};
use hamlet_host::Host;
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};

/// A counter kept in a SQL table. `increment` adds one in a
/// transaction, `discard` adds one and rolls it back, and `schedule`
/// arms a one-shot `increment` after the milliseconds in its
/// arguments.
async fn run_counter(start: ActorStart) -> Result<()> {
    let ActorStart {
        ctx, mut events, ..
    } = start;
    let sql = ctx.sql();
    sql.exec(
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
                let result = match name.as_str() {
                    "increment" => add_one(&ctx, true).await,
                    "discard" => add_one(&ctx, false).await,
                    "get" => read_count(&ctx).await,
                    "schedule" => {
                        let ms = u64::from_le_bytes(
                            args.try_into().map_err(|_| {
                                anyhow!("schedule takes a u64")
                            })?,
                        );
                        ctx.after(
                            Duration::from_millis(ms),
                            "increment",
                            &[],
                        )
                        .await?;
                        read_count(&ctx).await
                    }
                    other => Err(anyhow!("unknown action `{other}`")),
                };
                reply.send(
                    result.map(|count| count.to_le_bytes().to_vec()),
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

async fn add_one(
    ctx: &hamlet_actor_core::ActorContext,
    commit: bool,
) -> Result<i64> {
    let tx = ctx.sql().begin_transaction(None).await?;
    tx.execute(
        "INSERT INTO counter (id, value) VALUES (0, 1) ON CONFLICT \
         (id) DO UPDATE SET value = value + 1",
        None,
    )
    .await?;
    if commit {
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    read_count(ctx).await
}

async fn read_count(
    ctx: &hamlet_actor_core::ActorContext,
) -> Result<i64> {
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

fn factories() -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("counter".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    HashMap::from([(
        "counter".to_owned(),
        ActorFactory::new(config, |start| {
            Box::pin(run_counter(start))
        }),
    )])
}

async fn call(host: &Host, actor_id: &str, action: &str) -> i64 {
    call_with(host, actor_id, action, Vec::new()).await
}

async fn call_with(
    host: &Host,
    actor_id: &str,
    action: &str,
    args: Vec<u8>,
) -> i64 {
    let reply = host.action(actor_id, action, args).await.unwrap();
    i64::from_le_bytes(reply.try_into().unwrap())
}

/// Opens the actor's file as another process would, without waiting
/// for a lock.
fn probe(dir: &Path, kind: &str, key: &str) -> rusqlite::Result<i64> {
    let path =
        dir.join("actors").join(kind).join(format!("{key}.sqlite"));
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    conn.busy_timeout(Duration::ZERO)?;
    conn.query_row(
        "SELECT generation FROM _hamlet_actor",
        [],
        |row| row.get(0),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn starts_an_actor_by_its_kind_and_key() {
    let dir = Scratch::new("host-start");
    let host = Host::open(&dir, factories()).unwrap();

    let id = host.get_or_start("counter", "a").await.unwrap();
    assert_eq!(id, "counter/a");
    assert_eq!(call(&host, &id, "increment").await, 1);
    assert_eq!(host.get_or_start("counter", "a").await.unwrap(), id);
    assert_eq!(call(&host, &id, "increment").await, 2);

    let other = host.get_or_start("counter", "b").await.unwrap();
    assert_eq!(call(&host, &other, "get").await, 0);
    assert!(dir.join("actors/counter/a.sqlite").is_file());

    // The running actor holds its file in exclusive locking mode.
    assert!(probe(&dir, "counter", "a").is_err());

    for bad in ["", "../a", ".hidden", "a/b"] {
        assert!(host.get_or_start("counter", bad).await.is_err());
    }
    assert!(host.get_or_start("nobody", "a").await.is_err());
    host.stop_all().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_actor_closes_its_file_and_starts_again() {
    let dir = Scratch::new("host-stop");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    call(&host, &id, "increment").await;
    call(&host, &id, "increment").await;

    host.stop(&id, ShutdownKind::Sleep).await.unwrap();
    assert!(!host.is_running(&id).await);
    assert!(host.action(&id, "get", Vec::new()).await.is_err());
    assert_eq!(probe(&dir, "counter", "a").unwrap(), 1);

    host.get_or_start("counter", "a").await.unwrap();
    assert_eq!(call(&host, &id, "get").await, 2);
    host.stop_all().await.unwrap();
    assert_eq!(probe(&dir, "counter", "a").unwrap(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_host_is_refused_by_the_folder_lock() {
    let dir = Scratch::new("host-lock");
    let first = Host::open(&dir, factories()).unwrap();
    let id = first.get_or_start("counter", "a").await.unwrap();

    let Err(error) = Host::open(&dir, factories()) else {
        panic!("a second host opened the folder");
    };
    let message = error.to_string();
    assert!(message.contains("host.lock"), "{message}");
    assert_eq!(call(&first, &id, "increment").await, 1);

    first.stop_all().await.unwrap();
    drop(first);
    let again = Host::open(&dir, factories()).unwrap();
    let id = again.get_or_start("counter", "a").await.unwrap();
    assert_eq!(call(&again, &id, "get").await, 1);
    again.stop_all().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn boot_arms_a_stored_alarm_and_starts_the_actor_when_due() {
    let dir = Scratch::new("host-alarm");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    call_with(&host, &id, "schedule", 600u64.to_le_bytes().to_vec())
        .await;
    host.stop_all().await.unwrap();
    drop(host);

    let host = Host::open(&dir, factories()).unwrap();
    assert!(host.boot().await.unwrap().is_empty());
    assert!(!host.is_running(&id).await);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(host.is_running(&id).await);
    assert_eq!(call(&host, &id, "get").await, 1);
    host.stop_all().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn boot_starts_an_actor_whose_alarm_came_due_while_down() {
    let dir = Scratch::new("host-due");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    call_with(&host, &id, "schedule", 200u64.to_le_bytes().to_vec())
        .await;
    host.stop_all().await.unwrap();
    drop(host);
    tokio::time::sleep(Duration::from_millis(400)).await;

    let host = Host::open(&dir, factories()).unwrap();
    assert_eq!(host.boot().await.unwrap(), vec![id.clone()]);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(call(&host, &id, "get").await, 1);
    host.stop_all().await.unwrap();

    // The alarm fired once and is gone, so the next boot starts
    // nothing.
    drop(host);
    let host = Host::open(&dir, factories()).unwrap();
    assert!(host.boot().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_committed_transaction_tells_the_host_and_a_rollback_does_not()
 {
    let dir = Scratch::new("host-commit");
    let host = Host::open(&dir, factories()).unwrap();
    let id = host.get_or_start("counter", "a").await.unwrap();
    let mut commits = host.commits();
    while commits.try_recv().is_ok() {}

    assert_eq!(call(&host, &id, "discard").await, 0);
    assert!(commits.try_recv().is_err());
    assert_eq!(call(&host, &id, "increment").await, 1);
    assert_eq!(commits.try_recv().unwrap(), id);
    assert!(commits.try_recv().is_err());
    host.stop_all().await.unwrap();
}
