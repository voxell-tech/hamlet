//! A chief's oversight: the waits an agent closes, the spending the
//! realm holds, and the grants a chief takes back.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use super::{ChiefRequest, Done};
use crate::Message;
use crate::address::{Agent, Realm};
use crate::workspace::{AgentNumber, ConversationId, RealmTask};

/// Closes the agent's open waits in a conversation, or all of them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lapse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<ConversationId>,
    pub why: LapseReason,
}

impl Action for Lapse {
    type Output = ();
    const NAME: &'static str = "lapse";
}

impl Message for Lapse {
    type To = Realm;
    type From = Agent;
}

/// Why the agent's waits lapse.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum LapseReason {
    /// A chief stopped the agent's turn.
    Stopped,
    /// The conversation was taken back past the waits.
    Rewound,
    /// The agent ended.
    Ended,
    /// A chief went back to an earlier session, which ends the trust
    /// given for this one.
    Resumed,
}

/// Why the realm holds an agent's spending.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Hold {
    #[error(
        "task {} is held on an objection until a chief says go or \
         withdraw",
        task.0
    )]
    Objection { task: RealmTask },
}

/// Takes back an agent's grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revoke {
    pub agent: AgentNumber,
    pub permission: Permission,
}

impl ChiefRequest for Revoke {
    const NAME: &'static str = "chief_revoke";
    type Answer = Done<RevokeRefusal>;
}

/// What a chief grants an agent beyond its hamlet's mode.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Network,
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RevokeRefusal {
    #[error("`hire#{}` holds no such grant", agent.0)]
    NoGrant { agent: AgentNumber },
}
