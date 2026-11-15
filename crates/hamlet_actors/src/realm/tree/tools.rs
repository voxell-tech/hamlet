//! The tools a lead turned off for a hire: the `hire_tool_off` table
//! and `SetTools` (C16).

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::{
    Done, OnlyAHire, Refusable, SetTools, ToolName,
};
use hamlet_messages::workspace::AgentNumber;

use super::super::act::{Act, Decided, Decision};
use super::super::callers::Lead;
use super::super::calls::Made;
use super::super::change::{Affected, Rows, Writer, number};
use super::super::record;
use super::{Lineage, Status};

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
    type Output = Done<By::Why<OnlyAHire>>;
    type Loaded = Option<Lineage>;
    type Done = ();

    async fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> Result<Option<Lineage>> {
        super::named(rows, by, &self.hire).await
    }

    fn decide(
        &self,
        by: &By,
        lineage: Option<Lineage>,
    ) -> Decision<(), Self::Output> {
        let refuse =
            |why| Decision::Answer(Done::refused(By::refused(why)));
        let Some(lineage) = lineage else {
            return refuse(OnlyAHire::NoSuchAgent);
        };
        if lineage.agent.kind == AgentKind::Hand {
            return refuse(OnlyAHire::NotAHire);
        }
        let hire = lineage.agent.agent;
        if let Err(not_a_lead) = by.leads(&lineage, hire) {
            return Decision::Answer(Done::refused(not_a_lead));
        }
        if let Status::Ended(_) = lineage.agent.status {
            return refuse(OnlyAHire::NotAHire);
        }
        Decision::Change(
            Decided::new(())
                .change(super::Change::Tools {
                    hire,
                    off: self.off.clone(),
                    on: self.on.clone(),
                })
                .record(record::Entry::Tools { hire, by: by.by() }),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
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
    }
    Ok(affected)
}
