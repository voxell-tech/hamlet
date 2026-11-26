use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::address::Workspace;
use hamlet_messages::realm::HamletId;
use hamlet_messages::realm::oversight::Permission;
use hamlet_messages::workspace::reach::{
    Footing, Reach, SecretsVersion,
};
use hamlet_messages::workspace::{
    AgentNumber, InForce, Numbered, SetReach,
};

use super::change::{self, Rows, number};
use super::peer::When;
use super::{
    Realm, chiefs, dreams, oversight, secrets, tree, versions,
};

/// `agent`'s reach as the rows give it now, under its wanted version,
/// asking for the secrets `secrets` holds then; `Reach::Ended` for an
/// agent that ended. `folder` is the realm folder.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
    secrets: &secrets::Held,
    folder: &Path,
) -> Result<SetReach> {
    let (version, _) = versions::reach(rows, agent).await?;
    let secrets = secrets.version();
    let reach = match agent.numbered() {
        Numbered::Errand(n) => {
            dreams::reach(rows, n, folder, secrets).await?
        }
        _ => standing(rows, agent, secrets).await?,
    };
    Ok(SetReach {
        agent,
        version,
        reach,
    })
}

/// The reach of `agent`, not a dream's errand, as the rows give it
/// now, asking for the secrets of `secrets`.
pub(super) async fn standing(
    rows: &Rows<'_>,
    agent: AgentNumber,
    secrets: SecretsVersion,
) -> Result<Reach> {
    let worker = rows
        .row::<(Option<u32>, Option<String>)>(
            "SELECT starter, hamlet FROM worker WHERE agent = ?1",
            vec![number(agent)],
        )
        .await?;
    Ok(if tree::left(rows, agent).await?.is_some() {
        Reach::Ended
    } else if let Some((starter, hamlet)) = worker {
        let starter = starter.map(AgentNumber);
        Reach::Worker {
            starter,
            footing: worker_footing(
                rows,
                agent,
                starter,
                hamlet.map(HamletId),
                secrets,
            )
            .await?,
        }
    } else {
        Reach::Agent(oversight::footing(rows, agent, secrets).await?)
    })
}

/// Worker `agent`'s footing: its starter's without the trust, or for
/// a chief's worker, its hamlet's; with the network its own grant
/// gives.
async fn worker_footing(
    rows: &Rows<'_>,
    agent: AgentNumber,
    starter: Option<AgentNumber>,
    hamlet: Option<HamletId>,
    secrets: SecretsVersion,
) -> Result<Footing> {
    let mut footing = if let Some(starter) = starter {
        Footing {
            trust: Vec::new(),
            ..oversight::footing(rows, starter, secrets).await?
        }
    } else {
        let hamlet = match hamlet {
            Some(id) => chiefs::hamlet(rows, &id).await?,
            None => None,
        };
        oversight::hamlet_footing(rows, hamlet.as_ref(), secrets)
            .await?
    };
    footing.network |=
        oversight::holds(rows, agent, Permission::Network).await?;
    Ok(footing)
}

/// Carries `agent`'s wanted reach to the workspace and commits its
/// answer.
pub(super) async fn push(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    agent: AgentNumber,
) -> Result<()> {
    let set = of(
        &Rows::outside(ctx)?,
        agent,
        &realm.secrets,
        &realm.folder,
    )
    .await?;
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
            let realm = started.realm.clone();
            let set = started
                .race(
                    |ctx| async move {
                        let rows = Rows::outside(&ctx)?;
                        of(&rows, HIRE, &realm.secrets, &realm.folder)
                            .await
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
