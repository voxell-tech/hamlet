#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U3 fills this file"
)]

mod push;

use anyhow::Result;
use hamlet_messages::agent::Place;
use hamlet_messages::workspace::AgentNumber;
pub(super) use push::send;

use super::change::Rows;

/// `agent`'s place as the rows give it now, under its version.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Place> {
    todo!()
}
