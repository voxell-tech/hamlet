//! The world a client shows, as the realm answers a
//! [`super::client::ReadWorld`].

use hamlet_protocol::Effort;
use serde::{Deserialize, Serialize};

use super::budget::Payer;
use super::oversight::{Permission, Question};
use super::parcels::Measure;
use super::{HamletId, Left, ProfileId, Tier, ToolName};
use crate::workspace::files::EditDiff;
use crate::workspace::reach::Need;
use crate::workspace::{AgentNumber, CallId, ReachVersion};

/// The number of the last change a client can see. A client told of a
/// later one reads the world again.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub struct Revision(pub u64);

#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct Seen {
    pub revision: Revision,
    pub agents: Vec<AgentSeen>,
    pub hamlets: Vec<HamletSeen>,
    pub roads: Vec<RoadSeen>,
    pub waits: Vec<WaitSeen>,
    pub spend: Vec<Spend>,
    /// The grants in force, which a chief can take back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<GrantSeen>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSeen {
    pub agent: AgentNumber,
    /// Absent for the Hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hamlet: Option<HamletId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<AgentNumber>,
    pub status: Status,
    pub reach: ReachSeen,
    /// Who started it, for a worker; absent for any other agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<Starter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    /// The model it runs now; absent while none is chosen anywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<String>,
    /// The model chosen for it; absent while it follows its hamlet's
    /// or the realm's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast: Option<bool>,
    /// What a hire may spend in its hamlet's budget period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowance_micros: Option<u64>,
    /// The tools a chief turned off for it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_off: Vec<ToolName>,
}

/// Who started a worker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Starter {
    Agent { agent: AgentNumber },
    Chief,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Status {
    Hiring,
    Live,
    Ended { left: Left },
}

/// An agent's reach as the workspace holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReachSeen {
    /// The realm has set none yet.
    None,
    Applied {
        version: ReachVersion,
    },
    /// The workspace has not answered for `wanted` yet.
    Pending {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        applied: Option<ReachVersion>,
        wanted: ReachVersion,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HamletSeen {
    pub hamlet: HamletId,
    pub name: String,
    pub mayor: AgentNumber,
    pub tier: Tier,
}

/// A laid road, its lower end as `a`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoadSeen {
    pub a: AgentNumber,
    pub b: AgentNumber,
}

/// The realm's number of a wait for a chief.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct WaitId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitSeen {
    pub wait: WaitId,
    pub agent: AgentNumber,
    /// The words a chief reads.
    pub subject: String,
    pub kind: WaitKind,
}

/// What a wait asks a chief, and what the chief is shown to answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WaitKind {
    /// A call the workspace keeps until a chief allows it.
    Tool {
        call: CallId,
        /// What allowing it lets through.
        danger: Need,
        /// What allowing it always keeps; absent when `always` is
        /// not offered.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        always: Option<Lasting>,
        /// An edit's change.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        change: Option<EditDiff>,
    },
    Question {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        questions: Vec<Question>,
    },
    /// An objection to a chief's instruction, which holds its task.
    Objection {
        reason: String,
        source: ObjectionSource,
    },
    /// A lead's act a chief keeps for itself.
    Reserved,
    /// Files an agent sends that are more than it may send alone.
    Send { measure: Measure },
    /// A turn that ended with no answer.
    Fault,
    /// A profile an agent made, waiting at the harbor to be let into
    /// the city.
    Newcomer { profile: ProfileId },
}

/// What allowing a call always keeps letting through.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Lasting {
    /// Its kind of call, for the rest of the conversation's session.
    Session,
    /// A grant, until a chief takes it back.
    Grant,
}

/// Who raised an objection.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ObjectionSource {
    /// The agent working on the instruction.
    Agent,
    /// Its lead, reviewing the instruction.
    Review,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantSeen {
    pub agent: AgentNumber,
    pub permission: Permission,
    pub source: GrantSource,
}

/// Where a grant comes from.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum GrantSource {
    /// Its hamlet's limits.
    Limits,
    /// A chief's `always`.
    Grant,
}

/// A payer's spending in its period. A budget that bounds nothing is
/// absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spend {
    pub payer: Payer,
    pub spent_micros: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_micros: Option<u64>,
    pub spent_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_ms: Option<u64>,
}
