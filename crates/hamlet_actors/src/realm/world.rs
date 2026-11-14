//! The world a client reads: a copy in memory, rebuilt after each
//! commit from the parts it changed, and answered with no SQL, so it
//! answers while a transaction runs (S26). Its revision is
//! `versions`'.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U10 fills this file"
)]

use std::sync::{Arc, RwLock};

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::realm::client::ReadWorld;
use hamlet_messages::realm::world::{Revision, Seen};
use hamlet_messages::sender::Client;

use super::Realm;
use super::act::Look;
use super::change::Affected;

/// A part of what a client sees. A module's `apply` reports the parts
/// it changed; only those are rebuilt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Part {
    Agents,
    Hamlets,
    Roads,
    Waits,
    Spend,
    Reaches,
}

/// The world as of one revision.
pub(super) type World = Seen;

/// The copy, swapped whole.
#[derive(Default)]
pub(super) struct Held {
    current: RwLock<Arc<World>>,
}

impl Held {
    /// The copy as the rows give it at the realm's start.
    pub async fn load(ctx: &Ctx<Realm>) -> Result<Self> {
        Ok(Self::default())
    }

    pub fn current(&self) -> Arc<World> {
        todo!()
    }

    /// Rebuilds the parts `affected` names, as of `revision`, and
    /// swaps the copy in when its revision is newer. A failure is
    /// logged: the next change or the next start rebuilds it.
    pub async fn refresh(
        &self,
        ctx: &Ctx<Realm>,
        revision: Revision,
        affected: &Affected,
    ) {
    }
}

impl Look<Client> for ReadWorld {
    type Output = Seen;

    fn look(&self, by: &Client, world: &World) -> Seen {
        todo!()
    }
}
