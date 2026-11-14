//! The address directory: where the actors of each kind run.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use hamlet_actor_core::ActorFactory;
use hamlet_actor_core::error::ActorRuntime;

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

    /// The kinds whose actors run in this host.
    pub(crate) fn local_kinds(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(kind, place)| {
            let Place::Local(_) = place;
            kind.as_str()
        })
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
