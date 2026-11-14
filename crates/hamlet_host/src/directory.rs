//! The address directory: where the actors of each kind run.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use hamlet_actor_core::ActorFactory;
use hamlet_actor_core::error::ActorRuntime;

use crate::Address;
use crate::link::Port;

/// Where the actors of one kind run.
pub(crate) enum Place {
    /// In this host, each made by the kind's factory.
    Local(Arc<ActorFactory>),
    /// In the host at the other end of the link, through this port.
    Linked(Port),
}

/// Each actor kind a host knows, and where its actors run. A kind the
/// directory does not hold has no actors.
pub(crate) struct Directory(HashMap<String, Place>);

impl Directory {
    /// Each kind of `factories` runs in this host. Each kind of
    /// `linked.theirs()` runs in the other host, mapped to that port.
    /// Fails for a linked kind that is not a plain name, or that
    /// `factories` holds too.
    pub(crate) fn new(
        factories: HashMap<String, ActorFactory>,
        linked: Option<Port>,
    ) -> Result<Self> {
        let places = factories
            .into_iter()
            .map(|(kind, factory)| {
                (kind, Place::Local(Arc::new(factory)))
            })
            .collect();
        if let Some(port) = linked {
            let _ = port;
            todo!("U5, the host unit: the other host's kinds")
        }
        Ok(Self(places))
    }

    /// The kinds whose actors run in this host.
    pub(crate) fn local_kinds(&self) -> impl Iterator<Item = &str> {
        self.0.iter().filter_map(|(kind, place)| match place {
            Place::Local(_) => Some(kind.as_str()),
            Place::Linked(_) => None,
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

    /// The factory of the kind of `at`, for an operation that only
    /// the host running the actor can do. Fails with
    /// `actor.not_registered` for a kind no host of the link holds.
    /// Fails with `actor.invalid_operation`, naming `operation`, for
    /// a kind the other host runs: only a call and a create cross the
    /// link.
    pub(crate) fn factory(
        &self,
        at: &Address,
        operation: &'static str,
    ) -> Result<&Arc<ActorFactory>> {
        match self.place(at.kind())? {
            Place::Local(factory) => Ok(factory),
            Place::Linked(_) => {
                let _ = operation;
                todo!(
                    "U5, the host unit: only a call and a create \
                     cross"
                )
            }
        }
    }
}
