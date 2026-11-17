//! The looks a call's request waits on, which the loop must not do:
//! the files a `notify` or a `request` sends, measured for the gate
//! and then copied into the receiver's inbox (H30), and the file an
//! edit writes as it is now, for the chief to see the change. Each
//! look runs on a task and its finding is kept on its call's entity;
//! the runtime's request runs again and finds it, and the runtime
//! waits for its answer meanwhile.

use std::path::PathBuf;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::system::{Commands, Query, SystemParam};
use hamlet_protocol::client::ImageId;
use hamlet_store::ids::AgentId;

use super::CallId;
use crate::attachments::inbox::Inbox;
use crate::realm::workspace::serving::Lodging;
use crate::society::refusal::Refusal;
use crate::tools::builtin::ToolSession;
use crate::tools::builtin::parcel::To;

/// The files a parcel holds and their bytes, as measured.
pub(crate) type Measure = (usize, u64);

/// A copied parcel's captions, which say where each file landed, and
/// the images among them.
pub(crate) type Copy = (String, Vec<ImageId>);

/// A call whose looks found something: an entity until the call is
/// settled, with a component for each finding.
#[derive(Component)]
pub(crate) struct CallWithLooks(CallId);

/// The files a parcel would send, as measured.
#[derive(Component)]
pub(crate) struct Measured(Result<Measure, Refusal>);

/// The file an edit writes, as it was before.
#[derive(Component)]
pub(crate) struct Previewed(Result<Option<String>, String>);

/// The files a parcel sent, as copied.
#[derive(Component)]
pub(crate) struct Copied(Result<Copy, Refusal>);

/// The findings of the looks, as the handlers reach them.
#[derive(SystemParam)]
pub(crate) struct LookFindings<'w, 's> {
    list: Query<
        'w,
        's,
        (
            Entity,
            &'static CallWithLooks,
            Option<&'static Measured>,
            Option<&'static Previewed>,
            Option<&'static Copied>,
        ),
    >,
    commands: Commands<'w, 's>,
}

impl LookFindings<'_, '_> {
    fn of(
        &self,
        call: &CallId,
    ) -> Option<(
        Entity,
        Option<&Measured>,
        Option<&Previewed>,
        Option<&Copied>,
    )> {
        self.list
            .iter()
            .find(|(_, looked, ..)| looked.0 == *call)
            .map(|(entity, _, measured, previewed, copied)| {
                (entity, measured, previewed, copied)
            })
    }

    pub fn measured(
        &self,
        call: &CallId,
    ) -> Option<&Result<Measure, Refusal>> {
        Some(&self.of(call)?.1?.0)
    }

    pub fn previewed(
        &self,
        call: &CallId,
    ) -> Option<&Result<Option<String>, String>> {
        Some(&self.of(call)?.2?.0)
    }

    pub fn copied(
        &self,
        call: &CallId,
    ) -> Option<&Result<Copy, Refusal>> {
        Some(&self.of(call)?.3?.0)
    }

    /// Keeps what a look of `call` found, once the input's commands
    /// apply. Gives the agent whose lodge it wrote into, when it
    /// copied files.
    pub fn found(
        &mut self,
        call: CallId,
        found: Found,
    ) -> Option<AgentId> {
        let entity = match self.of(&call) {
            Some((entity, ..)) => entity,
            None => self.commands.spawn(CallWithLooks(call)).id(),
        };
        let mut entity = self.commands.entity(entity);
        match found {
            Found::Measured(measured) => {
                entity.insert(Measured(measured));
                None
            }
            Found::Previewed(previewed) => {
                entity.insert(Previewed(previewed));
                None
            }
            Found::Copied { receiver, sent } => {
                entity.insert(Copied(sent));
                Some(receiver)
            }
        }
    }

    /// `call` is settled: what was found for it goes.
    pub fn forget(&mut self, call: &CallId) {
        if let Some((entity, ..)) = self.of(call) {
            self.commands.entity(entity).despawn();
        }
    }
}

/// A thing to look at, with the session to look through.
pub(crate) enum Look {
    /// The files under `paths` a parcel would send.
    Measure {
        session: ToolSession,
        lodging: Option<Lodging>,
        paths: Vec<String>,
    },
    /// The file at `path`, as it is now.
    Preview {
        session: ToolSession,
        lodging: Option<Lodging>,
        path: String,
    },
    /// The files under `paths`, copied into `receiver`'s `inbox`.
    Copy {
        session: ToolSession,
        lodging: Option<Lodging>,
        paths: Vec<String>,
        inbox: Box<Inbox>,
        /// The folder of kept images.
        images: PathBuf,
        receiver: AgentId,
    },
}

/// A look's finding.
pub(crate) enum Found {
    Measured(Result<Measure, Refusal>),
    Previewed(Result<Option<String>, String>),
    Copied {
        receiver: AgentId,
        sent: Result<Copy, Refusal>,
    },
}

impl Look {
    /// Looks, with `secrets` taken out of what it says. A remote
    /// workspace has the lodge in step first.
    pub async fn run(self, secrets: Vec<String>) -> Found {
        let scrubbed = |error: anyhow::Error| Refusal::LookFailed {
            error: hamlet_sandbox::scrub::scrub_text(
                &secrets,
                &format!("{error:#}"),
            ),
        };
        let lodging = match &self {
            Self::Measure { lodging, .. }
            | Self::Preview { lodging, .. }
            | Self::Copy { lodging, .. } => lodging.clone(),
        };
        let unready = match lodging {
            Some(lodging) => {
                lodging.in_step().await.err().map(|error| {
                    scrubbed(error.context(
                        "The remote workspace could not take the \
                         lodge in",
                    ))
                })
            }
            None => None,
        };
        match self {
            Self::Measure { session, paths, .. } => {
                Found::Measured(match unready {
                    Some(refusal) => Err(refusal),
                    None => {
                        match session.parcel(&paths, None).await {
                            Ok(sent) => sent
                                .map(|sent| (sent.files, sent.bytes)),
                            Err(error) => Err(scrubbed(error)),
                        }
                    }
                })
            }
            Self::Preview { session, path, .. } => {
                Found::Previewed(match unready {
                    Some(refusal) => Err(refusal.to_string()),
                    None => session
                        .preview(&path)
                        .await
                        .map_err(|unread| unread.to_string()),
                })
            }
            Self::Copy {
                session,
                paths,
                mut inbox,
                images,
                receiver,
                ..
            } => {
                let sent = match unready {
                    Some(refusal) => Err(refusal),
                    None => {
                        let to = To {
                            inbox: &mut inbox,
                            images,
                        };
                        match session.parcel(&paths, Some(to)).await {
                            Ok(Ok(sent)) => {
                                let mut captions =
                                    inbox.captions("of this message");
                                if sent.skipped > 0 {
                                    captions.push_str(&format!(
                                        "\n[{} links inside the \
                                         folders were left out]",
                                        sent.skipped
                                    ));
                                }
                                Ok((captions, sent.images))
                            }
                            Ok(Err(refusal)) => Err(refusal),
                            Err(error) => Err(scrubbed(error)),
                        }
                    }
                };
                Found::Copied { receiver, sent }
            }
        }
    }
}
