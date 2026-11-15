//! The road rule: which agents have a road between them (H3, H14,
//! H30).

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U3 fills this file"
)]

use anyhow::Result;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::{HamletId, Named};
use hamlet_messages::workspace::AgentNumber;

use super::change::Rows;
use super::tree::Status;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Way {
    /// Between a lead and its hire, or the Hand and a mayor.
    Tree,
    Mayors,
    Laid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct End {
    pub agent: AgentNumber,
    pub status: Status,
    pub lead: Option<AgentNumber>,
    pub hamlet: Option<HamletId>,
    pub is_mayor: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ends {
    pub from: End,
    pub to: Option<End>,
    /// Whether an open laid road joins them.
    pub laid: bool,
}

impl Ends {
    pub fn way(&self) -> Option<Way> {
        todo!()
    }
}

pub(super) async fn ends(
    rows: &Rows<'_>,
    from: &Agent,
    to: &Named,
) -> Result<Ends> {
    todo!()
}

/// Every agent `agent` has a road to, with the way.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<(AgentNumber, Way)>> {
    todo!()
}
