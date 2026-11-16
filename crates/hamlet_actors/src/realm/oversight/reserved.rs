//! The society tools the limits reserve for a chief: kept in a wait
//! until one allows or denies them.

use anyhow::{Result, bail};
use hamlet_messages::realm::SocietyTool;
use hamlet_messages::realm::client::Answer;
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::Choice;
use serde::Serialize;

use super::super::act::Decided;
use super::super::change::Rows;
use super::super::waits::Ruling;

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

/// What a reserved act's wait adds to the chief's `answer` on wait
/// `wait`.
pub(in crate::realm) async fn ruling(
    _rows: &Rows<'_>,
    wait: WaitId,
    answer: &Answer,
) -> Result<Ruling> {
    if answer.0.choice == Choice::Allow {
        bail!(
            "not built yet: U4, so the reserved wait {} is not \
             allowed",
            wait.0
        );
    }
    Ok(Ruling::default())
}
