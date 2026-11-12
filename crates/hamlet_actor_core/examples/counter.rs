// This file was changed for Hamlet. See NOTICE in this crate.
//! Minimal counter actor built directly against `hamlet_actor_core`.
//!
//! Most applications should use the higher-level `hamlet_actor`
//! crate. This example shows the lower-level receive-loop API exposed
//! by `hamlet_actor_core`.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use anyhow::{Result, anyhow};
use ciborium::{from_reader, into_writer};
use hamlet_actor_core::host::Host;
use hamlet_actor_core::{
    ActorConfig, ActorEvent, ActorFactory, ActorStart,
    RequestSaveOpts, SerializeStateReason, StateDelta,
};

fn encode_count(count: i64) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    into_writer(&count, &mut out)?;
    Ok(out)
}

fn decode_count(bytes: &[u8]) -> Result<i64> {
    if bytes.is_empty() {
        return Ok(0);
    }
    from_reader(Cursor::new(bytes)).map_err(Into::into)
}

async fn run_counter(start: ActorStart) -> Result<()> {
    let ActorStart {
        ctx,
        snapshot,
        mut events,
        ..
    } = start;
    let mut count = snapshot
        .as_deref()
        .map(decode_count)
        .transpose()?
        .unwrap_or(0);
    let mut dirty = false;

    while let Some(event) = events.recv().await {
        match event {
            ActorEvent::Action {
                name, args, reply, ..
            } => match name.as_str() {
                "increment" => {
                    let delta = decode_count(&args).unwrap_or(1);
                    count += delta;
                    dirty = true;
                    ctx.request_save(RequestSaveOpts::default());
                    reply.send(Ok(encode_count(count)?));
                }
                "get" => {
                    reply.send(Ok(encode_count(count)?));
                }
                other => {
                    reply.send(Err(anyhow!(
                        "unknown action `{other}`"
                    )));
                }
            },
            ActorEvent::SerializeState {
                reason: SerializeStateReason::Save,
                reply,
            } => {
                reply.send(build_deltas(count, &mut dirty));
            }
            ActorEvent::RunGracefulCleanup { reply, .. } => {
                ctx.save_state(build_deltas(count, &mut dirty)?)
                    .await?;
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

fn build_deltas(
    count: i64,
    dirty: &mut bool,
) -> Result<Vec<StateDelta>> {
    if !*dirty {
        return Ok(Vec::new());
    }

    *dirty = false;
    Ok(vec![StateDelta::ActorState(encode_count(count)?)])
}

fn counter_factory() -> ActorFactory {
    ActorFactory::new(
        ActorConfig {
            name: Some("counter".to_owned()),
            ..ActorConfig::default()
        },
        |start| Box::pin(run_counter(start)),
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    let dir = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("usage: counter <folder>"))?;
    let host = Host::open(
        Path::new(&dir),
        HashMap::from([("counter".to_owned(), counter_factory())]),
    )?;
    host.boot().await?;
    let actor_id = host.get_or_start("counter", "default").await?;
    let count = host
        .action(&actor_id, "increment", encode_count(1)?)
        .await?;
    println!("count {}", decode_count(&count)?);
    host.stop_all().await
}
