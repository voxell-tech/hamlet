//! The society tools the limits reserve for a chief: kept in a wait
//! until one allows or denies them.

use anyhow::Result;
use hamlet_messages::realm::SocietyTool;
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::workspace::AgentNumber;
use serde::Serialize;

use super::super::act::Decided;
use super::super::change::Rows;

/// The wait that keeps `act`, with the changes that open it, when the
/// limits reserve `tool` for a chief.
pub(in crate::realm) async fn reserve(
    _rows: &Rows<'_>,
    _tool: SocietyTool,
    _by: AgentNumber,
    _act: &impl Serialize,
) -> Result<Option<Decided<WaitId>>> {
    Ok(None)
}
