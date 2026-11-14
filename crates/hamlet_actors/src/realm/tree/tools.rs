//! The tools a lead turned off for a hire: the `hire_tool_off` table
//! and `SetTools` (C16).

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U11 fills this file"
)]

use anyhow::Result;
use hamlet_messages::realm::{
    Done, SetTools, SocietyTool, ToolsRefusal,
};
use hamlet_messages::workspace::AgentNumber;

use super::super::act::{Act, Decision};
use super::super::callers::Lead;
use super::super::calls::Made;
use super::super::change::{Affected, Rows, Writer};
use super::Lineage;

/// Whether `agent`'s lead turned `tool` off for it.
pub(in crate::realm) async fn is_off(
    rows: &Rows<'_>,
    agent: AgentNumber,
    tool: SocietyTool,
) -> Result<bool> {
    Ok(false)
}

/// The tools turned off for `agent`, for its place.
pub(in crate::realm) async fn off_for(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<SocietyTool>> {
    Ok(Vec::new())
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
        todo!()
    }

    fn decide(
        &self,
        by: &By,
        lineage: Option<Lineage>,
    ) -> Decision<(), Self::Output> {
        todo!()
    }

    fn answer(&self, (): (), made: &Made) -> Self::Output {
        todo!()
    }
}

/// `tree::apply`'s arm for `Change::Tools`. Reports the hire's place.
pub(in crate::realm) async fn set(
    writer: &Writer<'_>,
    hire: AgentNumber,
    off: Vec<SocietyTool>,
    on: Vec<SocietyTool>,
) -> Result<Affected> {
    todo!()
}
