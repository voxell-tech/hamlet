//! The tree of hires: the `profile`, `agent`, `hire` and
//! `hire_tool_off` tables, and the lead's powers over them.

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
/// the root of its hamlet, its own hires, and its hamlet.
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

pub(super) async fn lineage(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<Lineage>> {
    todo!()
}

pub(super) async fn resolve(
    rows: &Rows<'_>,
    hamlet: Option<&HamletId>,
    named: &Named,
) -> Result<Option<AgentNumber>> {
    todo!()
}

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
/// first mayor 2.
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Of {
    Profile(ProfileId),
    /// A new profile, made for this hire alone (H38).
    Blank(ProfileId),
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// Profile 1 and the Hand as agent 1, `hiring`.
    Hand,
    /// A new agent, `hiring`, and its place in a hamlet's tree.
    Hire {
        agent: AgentNumber,
        hamlet: HamletId,
        lead: Option<AgentNumber>,
        of: Of,
        purpose: Option<String>,
    },
    /// The agent is live.
    Live { agent: AgentNumber },
    /// A hire's purpose, when it has none.
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
    Tools {
        hire: AgentNumber,
        off: Vec<SocietyTool>,
        on: Vec<SocietyTool>,
    },
}

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

async fn hand(writer: &Writer<'_>) -> Result<Affected> {
    todo!()
}

/// Reports the lead's place.
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

/// Reports its lead's place.
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
