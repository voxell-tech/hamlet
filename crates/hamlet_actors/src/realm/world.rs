//! The world a client reads: a copy in memory.

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Part {
    Agents,
    Hamlets,
    Roads,
    Waits,
    Spend,
    Reaches,
}

pub(super) type World = Seen;

#[derive(Default)]
pub(super) struct Held {
    current: RwLock<Arc<World>>,
}

impl Held {
    pub async fn load(ctx: &Ctx<Realm>) -> Result<Self> {
        Ok(Self::default())
    }

    pub fn current(&self) -> Arc<World> {
        todo!()
    }

    /// Rebuilds the parts `affected` names, as of `revision`, and
    /// swaps the copy in when its revision is newer.
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
