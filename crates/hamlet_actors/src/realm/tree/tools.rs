//! The tools a lead turned off for a hire: the `hire_tool_off` table
//! and `SetTools` (C16).

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::choices::SetTools;
use hamlet_messages::realm::{
    AgentName, OnlyAHire, Refusable, ToolName,
};
use hamlet_messages::workspace::AgentNumber;

use super::super::act::{Act, Decided, Decision};
use super::super::callers::Lead;
use super::super::calls::Made;
use super::super::change::{Affected, Rows, Writer, number};
use super::super::record;
use super::super::world::Part;
use super::Lineage;

pub(in crate::realm) async fn off_for(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<ToolName>> {
    Ok(rows
        .values::<String>(
            "SELECT tool FROM hire_tool_off WHERE hire = ?1 ORDER \
             BY tool",
            vec![number(agent)],
        )
        .await?
        .into_iter()
        .map(|name| ToolName { name })
        .collect())
}

impl<By: Lead> Act<By> for SetTools {
    type Output = By::OnAgent<By::Why<OnlyAHire>>;
    type Loaded = Option<Lineage>;
    type Done = AgentName;

    async fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> Result<Self::Loaded> {
        super::named(rows, by, &self.hire).await
    }

    fn decide(
        &self,
        by: &By,
        named: Self::Loaded,
    ) -> Decision<AgentName, Self::Output> {
        let refuse = |why| {
            Decision::Answer(Self::Output::refused(By::refused(why)))
        };
        let Some(lineage) = named else {
            return refuse(OnlyAHire::NoSuchAgent {
                address: self.hire.to_string(),
            });
        };
        if lineage.agent.kind != AgentKind::Hire {
            return refuse(OnlyAHire::NotAHire {
                name: lineage.agent.name.name.clone(),
            });
        }
        let hire = lineage.agent.agent;
        if let Err(not_a_lead) = by.leads(&lineage) {
            return Decision::Answer(Self::Output::refused(
                not_a_lead,
            ));
        }
        Decision::Change(
            Decided::new(lineage.agent.name.clone())
                .change(super::Change::Tools {
                    hire,
                    off: self.off.clone(),
                    on: self.on.clone(),
                })
                .record(record::Entry::Tools {
                    hire,
                    by: by.by(),
                    why: self.why.clone(),
                }),
        )
    }

    fn answer(&self, hire: AgentName, _made: &Made) -> Self::Output {
        By::on_agent(hire)
    }
}

pub(in crate::realm) async fn set(
    writer: &Writer<'_>,
    hire: AgentNumber,
    off: Vec<ToolName>,
    on: Vec<ToolName>,
) -> Result<Affected> {
    let mut changed = 0;
    for tool in off {
        changed += writer
            .execute(
                "INSERT INTO hire_tool_off (hire, tool) VALUES (?1, \
                 ?2) ON CONFLICT (hire, tool) DO NOTHING",
                vec![number(hire), BindParam::Text(tool.name)],
            )
            .await?;
    }
    for tool in on {
        changed += writer
            .execute(
                "DELETE FROM hire_tool_off WHERE hire = ?1 AND tool \
                 = ?2",
                vec![number(hire), BindParam::Text(tool.name)],
            )
            .await?;
    }
    let mut affected = Affected::default();
    if changed > 0 {
        affected.places.insert(hire);
        affected.seen.insert(Part::Agents);
    }
    Ok(affected)
}
