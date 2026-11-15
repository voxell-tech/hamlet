//! The kind `ticker`, an actor of the raw core whose clients follow
//! its event [`TICKED`].
//!
//! - `tick` broadcasts [`TICKED`] with the tick's number, from 1 in
//!   each generation, as a little-endian `u64`, then as many zero
//!   bytes as the little-endian `u64` its arguments hold, if any.
//! - `sleep`, `destroy` and `crash` ask the host to sleep, destroy or
//!   stop the actor with an error.
//! - `fail` ends the actor's event loop with an error.
//! - `hang` makes the next sleep or destroy hook wait for ever.
//! - `close` closes the connections open beside its own call's.
//! - `watchers` answers the callers of the connections open beside
//!   its own call's, as the CBOR of `Vec<Option<Caller>>`.
//!
//! A subscribe to the event `secret` is refused with
//! `auth.forbidden`, and one to `slow` takes [`WEIGHS`].

use std::collections::HashMap;
use std::time::Duration;

use anyhow::anyhow;
use hamlet_actor_core::{
    ActorConfig, ActorContext, ActorEvent, ActorFactory, ActorStart,
    ConnHandle,
};
use hamlet_actor_error::ActorError;
use hamlet_host::{Address, Caller, Host, Item};

pub const TICKED: &str = "ticked";

/// How long a subscribe to the event `slow` takes.
pub const WEIGHS: Duration = Duration::from_secs(3);

const DEADLINE: Duration = Duration::from_secs(10);

#[derive(ActorError)]
#[error("auth", "forbidden", "Not allowed to follow this event.")]
struct Forbidden;

/// The kind `ticker`, which sleeps once idle for `idle`.
pub fn factories(idle: Duration) -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("ticker".to_owned()),
        has_database: true,
        sleep_timeout: idle,
        ..ActorConfig::default()
    };
    HashMap::from([(
        "ticker".to_owned(),
        ActorFactory::new(config, |start| Box::pin(run(start))),
    )])
}

async fn run(start: ActorStart) -> anyhow::Result<()> {
    let ActorStart {
        ctx, mut events, ..
    } = start;
    let mut ticks = 0_u64;
    let mut hangs = false;
    let mut hung = Vec::new();
    while let Some(event) = events.recv().await {
        match event {
            ActorEvent::Action {
                name,
                args,
                conn,
                reply,
                ..
            } => {
                let answer = match name.as_str() {
                    "tick" => {
                        ticks += 1;
                        Ok(broadcast_tick(&ctx, ticks, &args))
                    }
                    "sleep" => ctx.sleep().map(|()| Vec::new()),
                    "destroy" => ctx.destroy().map(|()| Vec::new()),
                    "crash" => ctx
                        .stop_with_error("crash")
                        .map(|()| Vec::new()),
                    "fail" => {
                        reply.send(Ok(Vec::new()));
                        return Err(anyhow!(
                            "the ticker's loop failed"
                        ));
                    }
                    "hang" => {
                        hangs = true;
                        Ok(Vec::new())
                    }
                    "close" => ctx
                        .disconnect_conns(|other| {
                            !is_own(other, conn.as_ref())
                        })
                        .await
                        .map(|()| Vec::new()),
                    "watchers" => watching(&ctx, conn.as_ref()),
                    other => Err(anyhow!("unknown action `{other}`")),
                };
                reply.send(answer);
            }
            ActorEvent::SubscribeRequest {
                event_name,
                reply,
                ..
            } => match event_name.as_str() {
                "secret" => reply.send(Err(Forbidden.build())),
                "slow" => {
                    tokio::spawn(async move {
                        tokio::time::sleep(WEIGHS).await;
                        reply.send(Ok(()));
                    });
                }
                _ => reply.send(Ok(())),
            },
            // As the authoring layer answers it: the connection's
            // transport closes.
            ActorEvent::DisconnectConn {
                conn_id,
                reason,
                reply,
            } => {
                reply.send(
                    ctx.disconnect_conn_with_reason(conn_id, reason)
                        .await,
                );
            }
            ActorEvent::RunGracefulCleanup { reply, .. } if hangs => {
                hung.push(reply);
            }
            ActorEvent::SerializeState { reply, .. } => {
                reply.send(Ok(Vec::new()));
            }
            ActorEvent::RunGracefulCleanup { reply, .. }
            | ActorEvent::ConnectionPreflight { reply, .. }
            | ActorEvent::ConnectionOpen { reply, .. } => {
                reply.send(Ok(()));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Broadcasts tick `n`, padded as `args` asks, and answers `n`.
fn broadcast_tick(
    ctx: &ActorContext,
    n: u64,
    args: &[u8],
) -> Vec<u8> {
    let padding = args.first_chunk().map_or(0, |bytes| {
        usize::try_from(u64::from_le_bytes(*bytes)).unwrap()
    });
    let mut event = n.to_le_bytes().to_vec();
    event.resize(event.len() + padding, 0);
    ctx.broadcast(TICKED, &event);
    n.to_le_bytes().to_vec()
}

/// Whether `conn` is the connection of the call `own` belongs to.
fn is_own(conn: &ConnHandle, own: Option<&ConnHandle>) -> bool {
    own.is_some_and(|own| own.id() == conn.id())
}

fn watching(
    ctx: &ActorContext,
    own: Option<&ConnHandle>,
) -> anyhow::Result<Vec<u8>> {
    let callers = ctx
        .conns()
        .filter(|conn| !is_own(conn, own))
        .map(|conn| {
            ciborium::from_reader::<Option<Caller>, _>(
                conn.params().as_slice(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut bytes = Vec::new();
    ciborium::into_writer(&callers, &mut bytes)?;
    Ok(bytes)
}

pub async fn ask(
    host: &Host,
    at: &Address,
    action: &str,
) -> anyhow::Result<Vec<u8>> {
    host.call(at, action, Vec::new(), DEADLINE).await
}

/// Ticks the ticker at `at` with `padding` zero bytes after the
/// tick's number, and answers the number.
pub async fn tick(host: &Host, at: &Address, padding: u64) -> u64 {
    let reply = host
        .call(at, "tick", padding.to_le_bytes().to_vec(), DEADLINE)
        .await
        .unwrap();
    u64::from_le_bytes(reply.try_into().unwrap())
}

pub async fn watchers(
    host: &Host,
    at: &Address,
) -> Vec<Option<Caller>> {
    let reply = ask(host, at, "watchers").await.unwrap();
    ciborium::from_reader(reply.as_slice()).unwrap()
}

/// The tick's number an event carries, or `None` for a gap.
pub fn number(item: &Item) -> Option<u64> {
    match item {
        Item::Event(args) => {
            Some(u64::from_le_bytes(*args.first_chunk().unwrap()))
        }
        Item::Gap => None,
    }
}
