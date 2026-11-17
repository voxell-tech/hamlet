//! What each ready runtime is owed of its conversations, sent in
//! order: made or forked, cancelled, told its tools, then each kept
//! input.

use std::path::Path;

use anyhow::{Context, Result};
use hamlet_protocol::runtime::{
    self, ModelRef, Picture, ServerMessage,
};
use hamlet_store::Db;

use super::conversations::{self, Conversation};
use super::inputs::{self, Input};
use super::runtimes::{Pending, Phase, Runtimes};
use super::{Env, imports, models};
use crate::images::{self, Limits};
use crate::providers::Providers;

/// Sends runtime `index` what it is owed. With `again`, inputs it
/// admitted whose end was not heard are sent again, so it answers
/// them again. The images of an input are made to fit the model of
/// its conversation, from the folder `images`.
pub(crate) async fn flush(
    db: &mut Db,
    env: &Env,
    runtimes: &mut Runtimes,
    providers: &Providers,
    images: &Path,
    index: usize,
    again: bool,
) -> Result<()> {
    if runtimes.list[index].phase != Phase::Ready {
        return Ok(());
    }
    let id = runtimes.list[index].id.clone();
    for owed in conversations::of_runtime(db, &id)? {
        let Some(handle) = owed.handle.clone() else {
            make(db, env, runtimes, index, &owed).await?;
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
            let pictures = if write {
                Vec::new()
            } else {
                pictures(
                    db,
                    providers,
                    images,
                    &owed.model()?,
                    &input,
                )
                .await?
            };
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
                            images: pictures,
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

/// The images of `input` as `model` takes them; none when it takes
/// none now, and the captions in its words still say where they are.
async fn pictures(
    db: &mut Db,
    providers: &Providers,
    images: &Path,
    model: &ModelRef,
    input: &Input,
) -> Result<Vec<Picture>> {
    let mut ids = input
        .mark()?
        .images()
        .into_iter()
        .map(|image| image.id)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let Some(takes) = providers.images(db, model)? else {
        return Ok(Vec::new());
    };
    let limits = Limits::of(&takes);
    ids.truncate(limits.per_line as usize);
    images::pictures(images.to_owned(), ids, limits).await
}

/// Asks the runtime to make `owed`, or to fork it from its parent
/// once the parent is made.
async fn make(
    db: &mut Db,
    env: &Env,
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
            // One begun before a model was set up is made with the
            // model its agent runs now, and waits while there is
            // none: no runtime can make a conversation with no model.
            let mut model = owed.model()?;
            if model.id.is_empty() || model.provider.is_empty() {
                model = models::choose(
                    db,
                    &env.settings,
                    &env.overrides,
                    owed.agent,
                )?
                .model;
            }
            if model.id.is_empty() || model.provider.is_empty() {
                return Ok(());
            }
            let agent = u64::try_from(owed.agent.0)
                .context("an agent's id is positive")?;
            let kind = owed.kind.wire();
            let earlier = imports::earlier(db, owed.id)?;
            runtimes
                .request(index, pending, |v, id| {
                    ServerMessage::Create(runtime::Create {
                        agent,
                        earlier,
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
