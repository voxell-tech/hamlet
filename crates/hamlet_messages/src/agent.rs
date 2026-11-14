//! The calls an `agent` actor takes from the other actors, and their
//! answers.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use crate::workspace::{CallId, CommandEnd};

/// Tells the agent how a command ended that the workspace answered
/// `Running` for. Called by the workspace once, when it ends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finished {
    pub call: CallId,
    pub end: CommandEnd,
}

impl Action for Finished {
    type Output = ();
    const NAME: &'static str = "finished";
}
