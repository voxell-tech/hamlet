mod push;

use anyhow::Result;
use hamlet_messages::agent::{Place, PlaceVersion};
use hamlet_messages::workspace::AgentNumber;
pub(super) use push::send;

use super::change::Rows;
use super::{roads, tree, versions};

/// `agent`'s place as the rows give it now, under its version.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Place> {
    let version = PlaceVersion(versions::place(rows, agent).await?);
    let lineage = tree::lineage_of(rows, agent).await?;
    let lead = lineage.agent.lead;
    let folder = lineage
        .hamlet
        .as_ref()
        .and_then(|hamlet| hamlet.folder.clone());
    let (hires, roads) = roads::of(rows, agent)
        .await?
        .into_iter()
        .map(|(other, _)| other)
        .filter(|&other| Some(other) != lead)
        .partition::<Vec<_>, _>(|&other| {
            lineage.hires.iter().any(|hire| hire.agent == other)
        });
    Ok(Place {
        version,
        lead,
        hires,
        roads,
        tools_off: tree::tools::off_for(rows, agent).await?,
        folder,
    })
}

#[cfg(test)]
mod tests {
    use hamlet_messages::realm::HamletId;

    use super::super::testing::Started;
    use super::*;

    const HIRE: AgentNumber = AgentNumber(3);

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_place_is_never_older_than_its_version() {
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, AgentNumber(2)).await;
        started.hire(&hamlet, HIRE, Some(AgentNumber(2))).await;
        let sql = started.ctx.sql();
        sql.execute(
            "INSERT INTO place (agent, version) VALUES (3, 1)",
            None,
        )
        .await
        .unwrap();
        for _ in 0..200 {
            let before =
                versions::place(&started.rows(), HIRE).await.unwrap();
            let place = started
                .race(
                    |ctx| async move {
                        of(&Rows::outside(&ctx)?, HIRE).await
                    },
                    &[
                        "UPDATE hire SET lead = NULL WHERE agent = 3",
                        "UPDATE place SET version = version + 1 \
                         WHERE agent = 3",
                    ],
                )
                .await;
            assert!(
                place.version == PlaceVersion(before)
                    || place.lead.is_none(),
                "{place:?}: the lead before the move under the \
                 move's version, raised from {before}"
            );
            sql.execute(
                "UPDATE hire SET lead = 2 WHERE agent = 3",
                None,
            )
            .await
            .unwrap();
        }
        started.stop().await;
    }
}
