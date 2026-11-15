//! The world a client shows, as the realm answers a
//! [`super::client::ReadWorld`].

use serde::{Deserialize, Serialize};

use super::budget::Payer;
use super::{HamletId, Left, Tier};
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

/// The realm's number of a wait for a chief. A client names it in
/// decimal, as the `wait` of its answer.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct WaitId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitSeen {
    pub wait: WaitId,
    pub agent: AgentNumber,
    pub call: CallId,
    /// The call in words, as the workspace gave it.
    pub subject: String,
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
