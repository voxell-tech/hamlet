//! The calls an `agent` actor takes from the other actors, and their
//! answers.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use crate::workspace::files::ChangeAnswer;
use crate::workspace::{CallId, CallRefusal, CommandEnd};

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

/// The end of a call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    /// Its command ended.
    Ran(CommandEnd),
    /// Its change, which a chief allowed, was carried out or failed.
    Changed(ChangeAnswer),
    /// It did not run: a chief refused it, its wait lapsed, or what
    /// the chief allowed no longer holds.
    NotRun(CallRefusal),
}
