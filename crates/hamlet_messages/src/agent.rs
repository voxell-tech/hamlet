//! The calls an `agent` actor takes from the other actors, and their
//! answers.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use crate::Message;
use crate::address::{Agent, Workspace};
use crate::workspace::files::ChangeEnd;
use crate::workspace::{CallId, RunEnd};

/// Tells the agent how a call ended that the workspace answered
/// `Running` or `WaitsForChief` for. Called by the workspace once,
/// when it ends, unless the workspace's host stopped first. A call
/// that fails is not made again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finished {
    pub call: CallId,
    pub outcome: Outcome,
}

impl Action for Finished {
    type Output = ();
    const NAME: &'static str = "finished";
}

impl Message for Finished {
    type To = Agent;
    type From = Workspace;
}

/// The end of a call, by its kind: the `end` its answer would have
/// held under `ended`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    Run { end: RunEnd },
    Change { end: ChangeEnd },
}
