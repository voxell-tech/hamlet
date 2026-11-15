use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::address::Workspace;
use hamlet_messages::workspace::reach::{Footing, Project, Reach};
use hamlet_messages::workspace::{AgentNumber, InForce, SetReach};

use super::change::{self, Rows};
use super::peer::When;
use super::tree::{self, Status};
use super::{Realm, secrets, versions};

/// `agent`'s reach as the rows give it now, under its wanted version:
/// the hamlet's folder and mode, the agent's own filesystem, the
/// network setting; `Reach::Ended` for an agent that ended.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<SetReach> {
    let lineage = tree::lineage_of(rows, agent).await?;
    let reach = match lineage.agent.status {
        Status::Ended(_) => Reach::Ended,
        Status::Hiring | Status::Live => Reach::Agent(Footing {
            // With no folder, the agent works in its own filesystem.
            project: lineage.hamlet.and_then(|hamlet| {
                Some(Project {
                    folder: hamlet.folder?,
                    mode: hamlet.mode,
                })
            }),
            // No chief turns the network on yet (C19).
            network: false,
            trust: Vec::new(),
            secrets: secrets::VERSION,
        }),
    };
    let (version, _) = versions::reach(rows, agent).await?;
    Ok(SetReach {
        agent,
        version,
        reach,
    })
}

/// Carries `agent`'s wanted reach to the workspace and commits its
/// answer.
pub(super) async fn push(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    agent: AgentNumber,
) -> Result<()> {
    let set = of(&Rows::outside(ctx)?, agent).await?;
    let InForce { version } = realm
        .peers
        .call(&Workspace, &set, When::AfterAnswer)
        .await?;
    change::commit(
        realm,
        ctx,
        vec![versions::Change::Applied { agent, version }.into()],
    )
    .await?;
    Ok(())
}
