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
    })
}
