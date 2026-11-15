use std::sync::Arc;

use anyhow::{Context as _, Result};
use hamlet_actor::{BindParam, Ctx};
use hamlet_messages::address::Workspace;
use hamlet_messages::realm::HamletId;
use hamlet_messages::workspace::reach::{
    Footing, Project, Reach, SecretsVersion,
};
use hamlet_messages::workspace::{AgentNumber, InForce, SetReach};

use super::change::{self, Rows};
use super::peer::When;
use super::tree::{self, Status};
use super::{Realm, versions};

/// The version of the realm's list of secrets, which is empty.
const SECRETS: SecretsVersion = SecretsVersion(1);

/// `agent`'s reach as the rows give it now, under its wanted version:
/// the hamlet's folder and mode, the agent's own filesystem, the
/// network setting; `Reach::Ended` for an agent that ended.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<SetReach> {
    let lineage = tree::lineage(rows, agent)
        .await?
        .with_context(|| format!("agent {} has no row", agent.0))?;
    let reach = match lineage.agent.status {
        Status::Ended(_) => Reach::Ended,
        Status::Hiring | Status::Live => Reach::Agent(Footing {
            project: match &lineage.hamlet {
                Some(hamlet) => project(rows, &hamlet.id).await?,
                None => None,
            },
            // No chief turns the network on yet (C19).
            network: false,
            trust: Vec::new(),
            secrets: SECRETS,
        }),
    };
    let (version, _) = versions::reach(rows, agent).await?;
    Ok(SetReach {
        agent,
        version,
        reach,
    })
}

/// The folder `hamlet` works on, with its mode. Without one, an agent
/// works in its own filesystem.
async fn project(
    rows: &Rows<'_>,
    hamlet: &HamletId,
) -> Result<Option<Project>> {
    let found = rows
        .row::<(Option<String>, String)>(
            "SELECT folder, mode FROM hamlet WHERE id = ?1",
            vec![BindParam::Text(hamlet.0.clone())],
        )
        .await?;
    let Some((Some(folder), mode)) = found else {
        return Ok(None);
    };
    Ok(Some(Project {
        folder: folder.into(),
        mode: change::from_case_name(mode)?,
    }))
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
