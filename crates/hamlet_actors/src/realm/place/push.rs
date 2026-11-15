//! The push of a place: after a commit that moved an agent's lead,
//! its hires, its roads or its tools, the agent is sent its place as
//! the rows give it then. The agent keeps only a newer version, so a
//! push sent twice, or late, does no harm (bar §3.8).

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::address::Agent;
use hamlet_messages::workspace::AgentNumber;

use super::super::change::Rows;
use super::super::peer::When;
use super::super::{Realm, tree};

/// Sends `agent` its place. A failed call is the caller's to log: the
/// agent's copy stays old until the next change to its place. An
/// agent that has ended is sent nothing, since its place is moot.
pub(in crate::realm) async fn send(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    agent: AgentNumber,
) -> Result<()> {
    let rows = Rows::outside(ctx)?;
    if tree::left(&rows, agent).await?.is_some() {
        return Ok(());
    }
    let place = super::of(&rows, agent).await?;
    realm
        .peers
        .call(&Agent(agent), &place, When::AfterAnswer)
        .await
}

#[cfg(test)]
mod tests {
    use hamlet_messages::realm::HamletId;

    use super::super::super::record;
    use super::super::super::testing::Started;
    use super::*;

    const HIRE: AgentNumber = AgentNumber(3);

    #[tokio::test(flavor = "multi_thread")]
    async fn an_ended_agent_is_sent_no_place() {
        // the module's host has no agents, so a place sent to one
        // fails: the hire is sent its place, and once ended is not.
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, AgentNumber(2)).await;
        started.hire(&hamlet, HIRE, Some(AgentNumber(2))).await;
        let send =
            async || send(&started.realm, &started.ctx, HIRE).await;

        let hiring = send().await;
        started
            .commit(tree::Change::End {
                agent: HIRE,
                by: record::By::Chief,
                why: None,
            })
            .await
            .unwrap();
        let ended = send().await;

        assert!(hiring.is_err(), "a hiring hire is sent its place");
        ended.unwrap();
        started.stop().await;
    }
}
