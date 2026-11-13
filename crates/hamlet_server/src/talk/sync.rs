//! The conversations' state each ready runtime is owed, sent in
//! order: made or forked, cancelled, told its tools, then each kept
//! input. The images of an input are fitted to its model on a thread
//! of their own before it is sent.

use std::path::Path;

use anyhow::{Context, Result};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, Res, SystemParam};
use hamlet_protocol::runtime::{
    self, ModelRef, Picture, ServerMessage,
};
use hamlet_store::Db;
use hamlet_store::ids::ConversationId;
use tokio::sync::mpsc::UnboundedSender;

use super::conversations::{self, Conversation};
use super::inputs::{self, Input, Resend};
use super::runtimes::{Pending, Runtimes};
use super::{Env, models};
use crate::diagnostic::{self, Secrets, tell};
use crate::images::Limits;
use crate::images::fitting::pictures;
use crate::providers::Providers;

/// The fitting of images, whose results come back as inputs.
pub(crate) fn plugin(app: &mut bevy_app::App) {
    let done = crate::server::arriving::<Fitted>(app);
    app.insert_resource(FittedSender(done));
}

/// The images of one input, fitted to its model.
pub(crate) struct Fitted {
    pub conversation: ConversationId,
    pub request: String,
    pub pictures: Vec<Picture>,
}

/// The sender that brings fitted images back as inputs.
#[derive(Resource)]
pub(crate) struct FittedSender(UnboundedSender<Fitted>);

/// An input whose images are being fitted, or were and are not sent
/// yet: an entity until they are.
#[derive(Component)]
pub(crate) struct FittingImages {
    conversation: ConversationId,
    request: String,
    pictures: Option<Vec<Picture>>,
}

/// The inputs whose images are fitted, as the flushes reach them.
#[derive(SystemParam)]
pub(crate) struct Fitting<'w, 's> {
    done: Res<'w, FittedSender>,
    list: Query<'w, 's, (Entity, &'static mut FittingImages)>,
    commands: Commands<'w, 's>,
}

impl Fitting<'_, '_> {
    /// Keeps `fitted` for the next flush to send.
    pub fn fitted(&mut self, fitted: Fitted) {
        if let Some((_, mut found)) =
            self.list.iter_mut().find(|(_, found)| {
                found.conversation == fitted.conversation
                    && found.request == fitted.request
            })
        {
            found.pictures = Some(fitted.pictures);
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
        if let Some((entity, mut found)) =
            self.list.iter_mut().find(|(_, found)| {
                found.conversation == input.conversation
                    && found.request == input.request
            })
        {
            let pictures = found.pictures.take();
            if pictures.is_some() {
                self.commands.entity(entity).despawn();
            }
            return Ok(pictures);
        }
        let limits = Limits::of(&takes);
        ids.truncate(limits.per_line as usize);
        self.commands.spawn(FittingImages {
            conversation: input.conversation,
            request: input.request.clone(),
            pictures: None,
        });
        let done = self.done.0.clone();
        let folder = images.to_owned();
        let (conversation, request) =
            (input.conversation, input.request.clone());
        tokio::task::spawn_blocking(move || {
            // An image that panics the decoder is left out like one
            // that fails, so its input is not held for good.
            let pictures = std::panic::catch_unwind(|| {
                pictures(&folder, &ids, limits)
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

/// The state a flush reaches.
pub(crate) struct Owed<'a, 'w, 's, 'v, 't> {
    pub db: &'a mut Db,
    pub env: &'a Env,
    pub runtimes: &'a mut Runtimes<'w, 's>,
    pub providers: &'a Providers,
    pub fitting: &'a mut Fitting<'v, 't>,
    /// The folder the images are kept in.
    pub images: &'a Path,
}

/// Sends the runtime `to` what it is owed, and the inputs `resend`
/// names.
pub(crate) fn flush(
    owed: &mut Owed<'_, '_, '_, '_, '_>,
    to: Entity,
    resend: Resend,
) -> Result<()> {
    let Owed {
        db,
        env,
        runtimes,
        providers,
        fitting,
        images,
    } = owed;
    if !runtimes.is_ready(to) {
        return Ok(());
    }
    let id = runtimes.id(to)?;
    for owed in conversations::of_runtime(db, &id)? {
        let Some(handle) = owed.handle.clone() else {
            make(db, env, runtimes, to, &owed)?;
            continue;
        };
        // No later input reaches the runtime until the cancel is
        // acknowledged.
        if owed.cancelling {
            let pending = Pending::Cancel(owed.id);
            if !runtimes.is_in_flight(to, &pending) {
                runtimes.request(to, pending, |v, id| {
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
            if !runtimes.is_in_flight(to, &pending) {
                runtimes.request(to, pending, |v, id| {
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
        for input in inputs::owed(db, owed.id, resend)? {
            let pending =
                Pending::Submit(owed.id, input.request.clone());
            if runtimes.is_in_flight(to, &pending) {
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
            runtimes.request(to, pending, |v, id| {
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
    runtimes: &mut Runtimes<'_, '_>,
    to: Entity,
    owed: &Conversation,
) -> Result<()> {
    let pending = Pending::Create(owed.id);
    if runtimes.is_in_flight(to, &pending) {
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
            runtimes.request(to, pending, |v, id| {
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
            runtimes.request(to, pending, |v, id| {
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
