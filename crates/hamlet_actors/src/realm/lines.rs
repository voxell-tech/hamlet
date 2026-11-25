//! The chief's lines the realm keeps for their agents until they
//! take them, and each agent's report of its open inputs. Memory
//! only.

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::{Ctx, Handler};
use hamlet_messages::realm::lines::{LineIs, ReadLine, Working};
use hamlet_messages::sender::{Client, sender};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;

use super::Realm;
use super::act::Look;

#[derive(Default)]
pub(super) struct Lines {}

impl Lines {
    pub fn read(&self, _id: &RequestId) -> LineIs {
        todo!("not implemented")
    }

    pub fn working(&self, _agent: AgentNumber, _open: bool) {
        todo!("not implemented")
    }
}

impl Look<Client> for ReadLine {
    type Output = LineIs;

    fn look(&self, _by: &Client, realm: &Realm) -> LineIs {
        realm.lines.read(&self.0)
    }
}

impl Handler<Working> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        working: Working,
    ) -> Result<()> {
        let agent = sender(&ctx, &working)?;
        self.lines.working(agent.0, working.open);
        Ok(())
    }
}
