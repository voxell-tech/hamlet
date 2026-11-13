//! What each ready runtime is owed of its conversations, sent in
//! order: made or forked, cancelled, told its tools, then each kept
//! input. The images of an input are fitted to its model on a thread
//! of their own before it is sent.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use hamlet_protocol::runtime::{
    self, ModelRef, Picture, ServerMessage,
};
use hamlet_store::Db;
use hamlet_store::ids::ConversationId;
use tokio::sync::mpsc::UnboundedSender;

use super::conversations::{self, Conversation};
use super::inputs::{self, Input};
use super::runtimes::{Pending, Phase, Runtimes};
use super::{Env, models};
use crate::diagnostic::{self, Secrets, tell};
use crate::images::{self, Limits};
use crate::providers::Providers;

/// The images of one input, fitted to its model.
pub(crate) struct Fitted {
    pub conversation: ConversationId,
    pub request: String,
    pub pictures: Vec<Picture>,
}

/// The inputs whose images are being fitted, and those fitted and
/// not sent yet.
#[derive(bevy_ecs::resource::Resource)]
pub(crate) struct Fitting {
    inputs: HashMap<(ConversationId, String), Option<Vec<Picture>>>,
    done: UnboundedSender<Fitted>,
}

impl Fitting {
    /// Fitted images arrive on `done`.
    pub fn new(done: UnboundedSender<Fitted>) -> Self {
        Self {
            inputs: HashMap::new(),
            done,
        }
    }

    /// Keeps `fitted` for the next flush to send.
    pub fn fitted(&mut self, fitted: Fitted) {
        let key = (fitted.conversation, fitted.request);
        if let Some(pictures) = self.inputs.get_mut(&key) {
            *pictures = Some(fitted.pictures);
        }
    }

    /// The images of `input` as `model` takes them, or `None` while
    /// they are being fitted. None are sent when the model takes none
    /// now, and the captions in its words still say where they are.
    fn pictures(
        &mut self,
        db: &mut Db,
        providers: &Providers,
        images: &Path,
        model: &ModelRef,
        input: &Input,
    ) -> Result<Option<Vec<Picture>>> {
        let mut ids = input
            .mark()?
            .images()
            .into_iter()
            .map(|image| image.id)
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let Some(takes) = providers.images(db, model)? else {
            return Ok(Some(Vec::new()));
        };
        let key = (input.conversation, input.request.clone());
        match self.inputs.get(&key) {
            Some(Some(_)) => {
                return Ok(self.inputs.remove(&key).flatten());
            }
            Some(None) => return Ok(None),
            None => {}
        }
        let limits = Limits::of(&takes);
        ids.truncate(limits.per_line as usize);
        self.inputs.insert(key, None);
        let done = self.done.clone();
        let folder = images.to_owned();
        let (conversation, request) =
            (input.conversation, input.request.clone());
        tokio::task::spawn_blocking(move || {
            // An image that panics the decoder is left out like one
            // that fails, so its input is not held for good.
            let pictures = std::panic::catch_unwind(|| {
                images::pictures(&folder, &ids, limits)
            })
            .unwrap_or_else(|panic| {
                tell(
                    Secrets::Fixed,
                    format!(
                        "fitting images failed: {}",
                        diagnostic::panicked(&*panic)
                    ),
                );
                Vec::new()
            });
            let _ = done.send(Fitted {
                conversation,
                request,
                pictures,
            });
        });
        Ok(None)
    }
}

/// What a flush reaches.
pub(crate) struct Owed<'a> {
    pub db: &'a mut Db,
    pub env: &'a Env,
    pub runtimes: &'a mut Runtimes,
    pub providers: &'a Providers,
    pub fitting: &'a mut Fitting,
    /// The folder the images are kept in.
    pub images: &'a Path,
}

/// Sends runtime `index` what it is owed. With `again`, inputs it
/// admitted whose end was not heard are sent again, so it answers
/// them again.
pub(crate) fn flush(
    owed: &mut Owed<'_>,
    index: usize,
    again: bool,
) -> Result<()> {
    let Owed {
        db,
        env,
        runtimes,
        providers,
        fitting,
        images,
    } = owed;
    if runtimes.list[index].phase != Phase::Ready {
        return Ok(());
    }
    let id = runtimes.list[index].id.clone();
    for owed in conversations::of_runtime(db, &id)? {
        let Some(handle) = owed.handle.clone() else {
            make(db, env, runtimes, index, &owed)?;
            continue;
        };
        // No later input reaches the runtime until the cancel is
        // acknowledged.
        if owed.cancelling {
            let pending = Pending::Cancel(owed.id);
            if !runtimes.is_in_flight(index, &pending) {
                runtimes.request(index, pending, |v, id| {
                    ServerMessage::Cancel(runtime::Cancel {
                        conversation: handle,
                        id,
                        v,
                    })
                });
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
                runtimes.request(index, pending, |v, id| {
                    ServerMessage::Configure(runtime::Configure {
                        conversation: handle,
                        id,
                        tools,
                        v,
                    })
                });
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
                let model = owed.model()?;
                match fitting
                    .pictures(db, providers, images, &model, &input)?
                {
                    Some(pictures) => pictures,
                    // The inputs after it wait for its images.
                    None => break,
                }
            };
            runtimes.request(index, pending, |v, id| {
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
            });
        }
    }
    Ok(())
}

/// Asks the runtime to make `owed`, or to fork it from its parent
/// once the parent is made.
fn make(
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
            runtimes.request(index, pending, |v, id| {
                ServerMessage::Fork(runtime::Fork {
                    at,
                    conversation: handle,
                    id,
                    key,
                    v,
                })
            });
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
            runtimes.request(index, pending, |v, id| {
                ServerMessage::Create(runtime::Create {
                    agent,
                    kind,
                    id,
                    key,
                    model,
                    v,
                })
            });
        }
    }
    Ok(())
}
