//! The tools a lead turned off for a hire: the `hire_tool_off` table
//! and `SetTools` (C16).

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::{
    Done, Refusable, SetTools, SocietyTool, ToolsRefusal,
};
use hamlet_messages::workspace::AgentNumber;

use super::super::act::{Act, Decided, Decision};
use super::super::callers::Lead;
use super::super::calls::Made;
use super::super::change::{self, Affected, Rows, Writer, number};
use super::super::record;
use super::{Lineage, Status};

pub(in crate::realm) async fn is_off(
    rows: &Rows<'_>,
    agent: AgentNumber,
    tool: SocietyTool,
) -> Result<bool> {
    Ok(rows
        .value::<i64>(
            "SELECT 1 FROM hire_tool_off WHERE hire = ?1 AND tool = \
             ?2",
            vec![number(agent), column(tool)?],
        )
        .await?
        .is_some())
}

pub(in crate::realm) async fn off_for(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<SocietyTool>> {
    rows.values::<String>(
        "SELECT tool FROM hire_tool_off WHERE hire = ?1 ORDER BY \
         tool",
        vec![number(agent)],
    )
    .await?
    .into_iter()
    .map(change::from_case_name)
    .collect()
}

impl<By: Lead> Act<By> for SetTools {
    type Output = Done<By::Why<ToolsRefusal>>;
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
            return refuse(ToolsRefusal::NoSuchAgent);
        };
        if lineage.agent.kind == AgentKind::Hand || lineage.is_mayor()
        {
            return refuse(ToolsRefusal::NotAHire);
        }
        let hire = lineage.agent.agent;
        if let Err(not_a_lead) = by.leads(&lineage, hire) {
            return Decision::Answer(Done::refused(not_a_lead));
        }
        if let Status::Ended(_) = lineage.agent.status {
            return refuse(ToolsRefusal::NotAHire);
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

/// Turns `off` off, then `on` on, so a tool in both ends on.
pub(in crate::realm) async fn set(
    writer: &Writer<'_>,
    hire: AgentNumber,
    off: Vec<SocietyTool>,
    on: Vec<SocietyTool>,
) -> Result<Affected> {
    let mut changed = 0;
    for tool in off {
        changed += writer
            .execute(
                "INSERT INTO hire_tool_off (hire, tool) VALUES (?1, \
                 ?2) ON CONFLICT (hire, tool) DO NOTHING",
                vec![number(hire), column(tool)?],
            )
            .await?;
    }
    for tool in on {
        changed += writer
            .execute(
                "DELETE FROM hire_tool_off WHERE hire = ?1 AND tool \
                 = ?2",
                vec![number(hire), column(tool)?],
            )
            .await?;
    }
    let mut affected = Affected::default();
    if changed > 0 {
        affected.places.insert(hire);
    }
    Ok(affected)
}

fn column(tool: SocietyTool) -> Result<BindParam> {
    Ok(BindParam::Text(change::case_name(&tool)?))
}
