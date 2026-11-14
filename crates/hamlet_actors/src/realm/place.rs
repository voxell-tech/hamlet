//! Each agent's place, as the agent is told it: who it answers to,
//! who answers to it, its roads and its tools turned off.
//! The place is a function of the realm's rows; its version is
//! `versions`'.

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

/// `agent`'s place as the rows give it now, under its version. The
/// first copy goes in the agent's create input (step 6).
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Place> {
    todo!()
}
