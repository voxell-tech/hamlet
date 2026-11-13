//! The address directory: where the actors of each kind run.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use hamlet_actor_core::ActorFactory;
use hamlet_actor_core::error::ActorRuntime;

/// An actor's address: its kind, which the directory maps to where
/// the kind's actors run, and its key within the kind.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Address {
    pub kind: String,
    pub key: String,
}

impl Address {
    pub fn new(
        kind: impl Into<String>,
        key: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            key: key.into(),
        }
    }
}

/// Where the actors of one kind run.
pub(crate) enum Place {
    /// In this host, each made by the kind's factory.
    Local(Arc<ActorFactory>),
}

/// Each actor kind a host knows, and where its actors run. A kind the
/// directory does not hold has no actors.
pub(crate) struct Directory(HashMap<String, Place>);

impl Directory {
    /// Each kind runs in this host, made by its factory.
    pub(crate) fn local(
        factories: HashMap<String, ActorFactory>,
    ) -> Self {
        Self(
            factories
                .into_iter()
                .map(|(kind, factory)| {
                    (kind, Place::Local(Arc::new(factory)))
                })
                .collect(),
        )
    }

    /// Where the actors of `kind` run. Fails with
    /// `actor.not_registered` for a kind the directory does not hold.
    pub(crate) fn place(&self, kind: &str) -> Result<&Place> {
        self.0.get(kind).ok_or_else(|| {
            ActorRuntime::NotRegistered {
                actor_name: kind.to_owned(),
            }
            .build()
        })
    }
}
