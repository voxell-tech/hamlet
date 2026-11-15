#![expect(
    unused_variables,
    reason = "the scaffold: U9 fills this file"
)]

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::workspace::AgentNumber;

use super::super::Realm;

pub(in crate::realm) async fn send(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    agent: AgentNumber,
) -> Result<()> {
    Ok(())
}
