//! What a call's request waits on that the loop must not: the files a
//! `tell` or an `ask` sends, measured for the gate and then copied
//! into the receiver's inbox (H30), and what a file an edit writes
//! holds now, for the chief to see the change. Each look runs on a
//! task. What it finds is kept here, and the runtime's request runs
//! again and finds it; the runtime waits for its answer meanwhile.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy_ecs::resource::Resource;
use hamlet_protocol::client::ImageId;
use hamlet_store::ids::AgentId;

use super::CallId;
use crate::inbox::Inbox;
use crate::society::refusal::Refusal;
use crate::work::tools::{To, ToolSession};
use crate::workspace::serving::Lodging;

/// The files a parcel holds and their bytes, as measured.
pub(crate) type Measure = (usize, u64);

/// What a copied parcel answers: the captions that say where each
/// file landed, and the images among them.
pub(crate) type Copy = (String, Vec<ImageId>);

/// What the looks found, by call, until the call is settled.
#[derive(Resource, Default)]
pub(crate) struct Looks {
    measured: HashMap<CallId, Result<Measure, Refusal>>,
    previewed: HashMap<CallId, Result<Option<String>, String>>,
    copied: HashMap<CallId, Result<Copy, Refusal>>,
}

impl Looks {
    pub fn measured(
        &self,
        call: &CallId,
    ) -> Option<&Result<Measure, Refusal>> {
        self.measured.get(call)
    }

    pub fn previewed(
        &self,
        call: &CallId,
    ) -> Option<&Result<Option<String>, String>> {
        self.previewed.get(call)
    }

    pub fn copied(
        &self,
        call: &CallId,
    ) -> Option<&Result<Copy, Refusal>> {
        self.copied.get(call)
    }

    /// Keeps what a look of `call` found. Gives the agent whose lodge
    /// it wrote into, when it copied files.
    pub fn found(
        &mut self,
        call: CallId,
        found: Found,
    ) -> Option<AgentId> {
        match found {
            Found::Measured(measured) => {
                self.measured.insert(call, measured);
                None
            }
            Found::Previewed(previewed) => {
                self.previewed.insert(call, previewed);
                None
            }
            Found::Copied { receiver, sent } => {
                self.copied.insert(call, sent);
                Some(receiver)
            }
        }
    }

    /// `call` is settled: what was found for it goes.
    pub fn forget(&mut self, call: &CallId) {
        self.measured.remove(call);
        self.previewed.remove(call);
        self.copied.remove(call);
    }
}

/// What to look at, and through which session.
pub(crate) enum Look {
    /// The files under `paths` a parcel would send.
    Measure {
        session: ToolSession,
        lodging: Option<Lodging>,
        paths: Vec<String>,
    },
    /// What the file at `path` holds now.
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

/// What a look found.
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
