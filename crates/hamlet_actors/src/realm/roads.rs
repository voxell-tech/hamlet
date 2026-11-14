//! The road rule: which agents have a road between them (H3, H14,
//! H30). A road follows from the tree (a lead and its hire, the Hand
//! and a mayor), from the hamlets (two mayors), or from a laid road
//! (`laid.rs`). The place an agent is sent lists its roads, and a
//! letter checks its road, both from here.

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

/// The way a letter goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Way {
    /// Between a lead and its hire, or the Hand and a mayor.
    Tree,
    /// Between two mayors.
    Mayors,
    /// Along a road a lead laid.
    Laid,
}

/// One end of a road, as its rows are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct End {
    pub agent: AgentNumber,
    pub status: Status,
    pub lead: Option<AgentNumber>,
    pub hamlet: Option<HamletId>,
    pub is_mayor: bool,
}

/// The two ends of a letter and whether a laid road joins them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ends {
    /// The caller, which the Tool form has already found a live
    /// member of the tree.
    pub from: End,
    /// `None` when the address names no agent.
    pub to: Option<End>,
    /// Whether an open laid road joins them.
    pub laid: bool,
}

impl Ends {
    /// The way from one end to the other, when one exists (H30).
    pub fn way(&self) -> Option<Way> {
        todo!()
    }
}

/// The ends of a letter from `from` to `to`.
pub(super) async fn ends(
    rows: &Rows<'_>,
    from: &Agent,
    to: &Named,
) -> Result<Ends> {
    todo!()
}

/// Every agent `agent` has a road to, with the way: for its place.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<(AgentNumber, Way)>> {
    todo!()
}
