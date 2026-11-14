//! The tree of hires: the `profile`, `agent`, `hire` and
//! `hire_tool_off` tables, and the lead's powers over them. This
//! module is the tables' one writer; `lead.rs` (hire, end, seat) and
//! `tools.rs` (a lead's tools turned off) are parts of it.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U2 fills this file"
)]

mod lead;
pub(super) mod tools;

use anyhow::Result;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::{
    HamletId, Left, Named, ProfileId, SocietyTool,
};
use hamlet_messages::workspace::AgentNumber;

use super::change::{Affected, Rows, Writer};
use super::{chiefs, record};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Hiring,
    Live,
    Ended(Left),
}

/// An agent of the tree, as its rows are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Member {
    pub agent: AgentNumber,
    pub kind: AgentKind,
    pub status: Status,
    /// `None` for the Hand and for a mayor.
    pub lead: Option<AgentNumber>,
    pub has_purpose: bool,
}

/// One agent with what a decision about it reads: its leads up to
/// the root of its hamlet, its own hires, and its hamlet. A lead's
/// powers, an end and a seating are decided on it. It is the depth of
/// the tree plus one level of hires, not the hamlet's tree, so a
/// commit stays small.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Lineage {
    pub agent: Member,
    /// Its lead first, the mayor last.
    pub leads: Vec<Member>,
    pub hires: Vec<Member>,
    /// `None` for the Hand.
    pub hamlet: Option<chiefs::Hamlet>,
}

impl Lineage {
    /// Whether `lead` is the agent's lead, or a lead of its leads, or
    /// the Hand (H41). An ended lead leads nothing.
    pub fn is_led_by(&self, lead: AgentNumber) -> bool {
        todo!()
    }

    pub fn is_mayor(&self) -> bool {
        todo!()
    }
}

/// `agent` with its leads, its hires and its hamlet; `None` when no
/// agent has that number.
pub(super) async fn lineage(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<Lineage>> {
    todo!()
}

/// The agent `named` is, for a caller in `hamlet` (H14): `hand`,
/// `mayor` (the office, read now), `mayor@<hamlet>`, `hire#<n>`.
pub(super) async fn resolve(
    rows: &Rows<'_>,
    hamlet: Option<&HamletId>,
    named: &Named,
) -> Result<Option<AgentNumber>> {
    todo!()
}

/// How and why `agent` ended, when it has.
pub(super) async fn left(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<Left>> {
    todo!()
}

/// The hires of `hamlet` that count toward its tier: `hiring` and
/// `live`.
pub(super) async fn living(
    rows: &Rows<'_>,
    hamlet: &HamletId,
) -> Result<u32> {
    todo!()
}

/// The next agent's number, from one sequence: the Hand is 1, the
/// first mayor 2. Read in the transaction that writes it, which holds
/// the file's gate from its read to its commit.
pub(super) async fn fresh(rows: &Rows<'_>) -> Result<AgentNumber> {
    todo!()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Profile {
    pub id: ProfileId,
}

pub(super) async fn profile(
    rows: &Rows<'_>,
    id: ProfileId,
) -> Result<Option<Profile>> {
    todo!()
}

/// The next profile's id: the Hand's is 1.
pub(super) async fn fresh_profile(
    rows: &Rows<'_>,
) -> Result<ProfileId> {
    todo!()
}

/// The profile `agent` was hired from: the Hand's is 1.
pub(super) async fn profile_of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<ProfileId> {
    todo!()
}

pub(super) async fn kind_of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<AgentKind> {
    todo!()
}

/// The profile a hire is of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Of {
    Profile(ProfileId),
    /// A new profile, made for this hire alone (H38).
    Blank(ProfileId),
}

/// The changes to this module's tables.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// Profile 1 and the Hand as agent 1, `hiring`, at the first
    /// start.
    Hand,
    /// A new agent, `hiring`, and its place in a hamlet's tree: a
    /// founded hamlet's mayor (no lead) or a hire.
    Hire {
        agent: AgentNumber,
        hamlet: HamletId,
        lead: Option<AgentNumber>,
        of: Of,
        purpose: Option<String>,
    },
    /// Step 9: the agent is live.
    Live { agent: AgentNumber },
    /// A hire's purpose, when it has none: the first line said to it.
    Purpose { hire: AgentNumber, text: String },
    /// The agent ends, and its hires go to its lead.
    End {
        agent: AgentNumber,
        by: record::By,
        why: Option<String>,
    },
    /// `mayor` becomes the root of its hamlet's tree, with the old
    /// mayor under it.
    Seat { mayor: AgentNumber },
    /// A lead turns tools off and on for its hire.
    Tools {
        hire: AgentNumber,
        off: Vec<SocietyTool>,
        on: Vec<SocietyTool>,
    },
}

/// Makes `change`, and reports the agents whose place or reach it
/// moved and whether a client sees it.
pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Hand => hand(writer).await,
        Change::Hire {
            agent,
            hamlet,
            lead,
            of,
            purpose,
        } => hire(writer, agent, hamlet, lead, of, purpose).await,
        Change::Live { agent } => live(writer, agent).await,
        Change::Purpose { hire, text } => {
            purpose(writer, hire, text).await
        }
        Change::End { agent, by, why } => {
            lead::end(writer, agent, by, why).await
        }
        Change::Seat { mayor } => lead::seat(writer, mayor).await,
        Change::Tools { hire, off, on } => {
            tools::set(writer, hire, off, on).await
        }
    }
}

/// Profile 1 and the Hand as agent 1, `hiring`.
async fn hand(writer: &Writer<'_>) -> Result<Affected> {
    todo!()
}

/// A new agent, `hiring`, under `lead`. Reports the lead's place.
async fn hire(
    writer: &Writer<'_>,
    agent: AgentNumber,
    hamlet: HamletId,
    lead: Option<AgentNumber>,
    of: Of,
    purpose: Option<String>,
) -> Result<Affected> {
    todo!()
}

/// The agent is live. Reports its lead's place.
async fn live(
    writer: &Writer<'_>,
    agent: AgentNumber,
) -> Result<Affected> {
    todo!()
}

async fn purpose(
    writer: &Writer<'_>,
    hire: AgentNumber,
    text: String,
) -> Result<Affected> {
    todo!()
}
