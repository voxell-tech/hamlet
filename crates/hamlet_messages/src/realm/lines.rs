//! A chief's lines, which the realm keeps from its answer to `say`
//! until their agent takes them, and the stop of agents, which is one
//! act of the realm.

use hamlet_actor::Action;
use hamlet_protocol::client::RequestId;
use serde::{Deserialize, Serialize};

use crate::Message;
use crate::address::{Agent, Realm};
use crate::agent::chat::Stopped;
use crate::sender::Client;
use crate::workspace::AgentNumber;

/// Asks what became of the chief's line said under the id `0`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReadLine(pub RequestId);

impl Action for ReadLine {
    type Output = LineIs;
    const NAME: &'static str = "read_line";
}

impl Message for ReadLine {
    type To = Realm;
    type From = Client;
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum LineIs {
    /// The realm keeps it for its agent.
    Waiting,
    /// Its agent took it.
    Given,
    /// It will not reach its agent: a stop withdrew it, or its agent
    /// left or refused it. A line the realm does not know reads so
    /// too.
    Withdrawn,
}

/// Stops the agents `agents` in one act: it withdraws the lines the
/// realm keeps for them, lapses their waits, and stops each one's
/// running turns after the lines it was given. It answers the turns
/// stopped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopAgents {
    pub agents: Vec<AgentNumber>,
}

impl Action for StopAgents {
    type Output = Stopped;
    const NAME: &'static str = "stop_agents";
}

impl Message for StopAgents {
    type To = Realm;
    type From = Client;
}

/// Tells the realm whether the agent has an open input, each time
/// that changes.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct Working {
    pub open: bool,
}

impl Action for Working {
    type Output = ();
    const NAME: &'static str = "working";
}

impl Message for Working {
    type To = Realm;
    type From = Agent;
}
