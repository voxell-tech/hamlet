//! The route table: the actor kinds each caller may reach.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::Caller;

/// A caller as the route table names it.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CallerKind {
    /// An actor of this kind.
    Actor(String),
    /// A client of the program that opened the host.
    Client,
}

impl CallerKind {
    /// The kind of `caller`.
    pub(crate) fn of(caller: &Caller) -> Self {
        match caller {
            Caller::Actor(from) => Self::Actor(from.kind.clone()),
            Caller::Client => Self::Client,
        }
    }
}

/// As a refusal names a caller: an actor by its kind, a client as
/// `client`.
impl std::fmt::Display for CallerKind {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Actor(kind) => f.write_str(kind),
            Self::Client => f.write_str("client"),
        }
    }
}

/// The (caller, target kind) pairs a host lets through. A call, a
/// create or a get-or-start with a caller passes only for a pair the
/// table holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Routes {
    /// Every pair.
    Any,
    /// These pairs only.
    Only(BTreeSet<(CallerKind, String)>),
}

impl Routes {
    /// The pairs of `table`: each caller with the kinds it may reach.
    pub fn only(table: &[(CallerKind, &[&str])]) -> Self {
        Self::Only(
            table
                .iter()
                .flat_map(|(from, kinds)| {
                    kinds.iter().map(|kind| {
                        (from.clone(), (*kind).to_owned())
                    })
                })
                .collect(),
        )
    }

    /// Whether the table lets `caller` reach an actor of `kind`.
    pub(crate) fn allows(&self, caller: &Caller, kind: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Only(pairs) => pairs
                .contains(&(CallerKind::of(caller), kind.to_owned())),
        }
    }
}
