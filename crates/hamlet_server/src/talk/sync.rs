//! What each ready runtime is owed of its conversations, sent in
//! order: made or forked, cancelled, told its tools, then each kept
//! input.

use anyhow::{Context, Result};
use hamlet_protocol::runtime::{self, ServerMessage};
use hamlet_store::Db;

use super::conversations::{self, Conversation};
use super::inputs;
use super::runtimes::{Pending, Phase, Runtimes};

/// Sends runtime `index` what it is owed. With `again`, inputs it
/// admitted whose end was not heard are sent again, so it answers
/// them again.
pub(crate) async fn flush(
    db: &mut Db,
    runtimes: &mut Runtimes,
    index: usize,
    again: bool,
) -> Result<()> {
    if runtimes.list[index].phase != Phase::Ready {
        return Ok(());
    }
    let id = runtimes.list[index].id.clone();
    for owed in conversations::of_runtime(db, &id)? {
        let Some(handle) = owed.handle.clone() else {
            make(db, runtimes, index, &owed).await?;
            continue;
        };
        // No later input reaches the runtime until the cancel is
        // acknowledged.
        if owed.cancelling {
            let pending = Pending::Cancel(owed.id);
            if !runtimes.is_in_flight(index, &pending) {
                runtimes
                    .request(index, pending, |v, id| {
                        ServerMessage::Cancel(runtime::Cancel {
                            conversation: handle,
                            id,
                            v,
                        })
                    })
                    .await;
            }
            continue;
        }
        // The tools a conversation offers reach the runtime before
        // the inputs kept after they changed.
        if let Some(tools) = owed.tools()?
            && owed.tools.as_deref() != owed.tools_told.as_deref()
        {
            let told = owed.tools.clone().unwrap_or_default();
            let pending = Pending::Configure(owed.id, told);
            if !runtimes.is_in_flight(index, &pending) {
                runtimes
                    .request(index, pending, |v, id| {
                        ServerMessage::Configure(runtime::Configure {
                            conversation: handle,
                            id,
                            tools,
                            v,
                        })
                    })
                    .await;
            }
            continue;
        }
        for input in inputs::owed(db, owed.id, again)? {
            let pending =
                Pending::Submit(owed.id, input.request.clone());
            if runtimes.is_in_flight(index, &pending) {
                continue;
            }
            let mark = input.mark()?.wire();
            let conversation = handle.clone();
            let write = input.is_write();
            runtimes
                .request(index, pending, |v, id| {
                    if write {
                        ServerMessage::Write(runtime::Write {
                            boundary: input.boundary.then_some(true),
                            conversation,
                            id,
                            mark,
                            request: input.request,
                            v,
                        })
                    } else {
                        ServerMessage::Submit(runtime::Submit {
                            content: input.content,
                            conversation,
                            id,
                            mark: Some(mark),
                            request: input.request,
                            steer: input.steer,
                            v,
                            window: None,
                        })
                    }
                })
                .await;
        }
    }
    Ok(())
}

/// Asks the runtime to make `owed`, or to fork it from its parent
/// once the parent is made.
async fn make(
    db: &mut Db,
    runtimes: &mut Runtimes,
    index: usize,
    owed: &Conversation,
) -> Result<()> {
    let pending = Pending::Create(owed.id);
    if runtimes.is_in_flight(index, &pending) {
        return Ok(());
    }
    let key = owed.id.to_string();
    match (owed.fork_of, &owed.fork_at) {
        (Some(parent), Some(at)) => {
            let Some(handle) = conversations::get(db, parent)?.handle
            else {
                return Ok(());
            };
            let at = at.clone();
            runtimes
                .request(index, pending, |v, id| {
                    ServerMessage::Fork(runtime::Fork {
                        at,
                        conversation: handle,
                        id,
                        key,
                        v,
                    })
                })
                .await;
        }
        _ => {
            let model = owed.model()?;
            let agent = u64::try_from(owed.agent.0)
                .context("an agent's id is positive")?;
            let kind = owed.kind.wire();
            runtimes
                .request(index, pending, |v, id| {
                    ServerMessage::Create(runtime::Create {
                        agent,
                        kind,
                        id,
                        key,
                        model,
                        v,
                    })
                })
                .await;
        }
    }
    Ok(())
}
