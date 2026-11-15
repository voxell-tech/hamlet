use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::address::Workspace;
use hamlet_messages::workspace::reach::{Footing, Project, Reach};
use hamlet_messages::workspace::{AgentNumber, InForce, SetReach};

use super::change::{self, Rows};
use super::peer::When;
use super::tree::{self, Status};
use super::{Realm, secrets, settings, versions};

/// `agent`'s reach as the rows give it now, under its wanted version:
/// the hamlet's folder and mode, the agent's own filesystem, the
/// network setting; `Reach::Ended` for an agent that ended.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<SetReach> {
    let (version, _) = versions::reach(rows, agent).await?;
    let lineage = tree::lineage_of(rows, agent).await?;
    let network = settings::read(rows).await?.allow_net;
    let reach = match lineage.agent.status {
        Status::Ended(_) => Reach::Ended,
        Status::Hiring | Status::Live => Reach::Agent(Footing {
            project: lineage.hamlet.and_then(|hamlet| {
                Some(Project {
                    folder: hamlet.folder?,
                    mode: hamlet.mode,
                })
            }),
            network,
            trust: Vec::new(),
            secrets: secrets::VERSION,
        }),
    };
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

#[cfg(test)]
mod tests {
    use hamlet_messages::realm::{HamletId, HowEnded};

    use super::super::testing::Started;
    use super::*;

    const HIRE: AgentNumber = AgentNumber(2);

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_reach_is_never_older_than_its_version() {
        let started = Started::new().await;
        started.found(&HamletId("h".to_owned()), HIRE).await;
        started
            .commit(versions::Change::WantReach(HIRE))
            .await
            .unwrap();
        let how =
            serde_json::to_string(&HowEnded::ByTheChief).unwrap();
        let end = format!(
            "UPDATE agent SET status = 'ended', ended_how = '{how}' \
             WHERE id = 2"
        );
        for _ in 0..200 {
            let (before, _) =
                versions::reach(&started.rows(), HIRE).await.unwrap();
            let set = started
                .race(
                    |ctx| async move {
                        of(&Rows::outside(&ctx)?, HIRE).await
                    },
                    &[
                        &end,
                        "UPDATE reach SET wanted = wanted + 1 WHERE \
                         agent = 2",
                    ],
                )
                .await;
            assert!(
                set.version == before || set.reach == Reach::Ended,
                "{set:?}: a footing under the end's version, raised \
                 from {before:?}"
            );
            started
                .ctx
                .sql()
                .execute(
                    "UPDATE agent SET status = 'hiring', ended_how \
                     = NULL WHERE id = 2",
                    None,
                )
                .await
                .unwrap();
        }
        started.stop().await;
    }
}
