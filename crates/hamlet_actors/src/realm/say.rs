//! A chief's line to an agent. It answers after its call,
//! saying whether the agent took the line.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U5 fills this file"
)]

use anyhow::Result;
use hamlet_messages::realm::client::{Say, SayAnswer};
use hamlet_messages::sender::Client;

use super::act::{Act, Decision};
use super::calls::{Calls, Made};
use super::change::Rows;
use super::tasks::TaskId;
use super::tree::Lineage;

/// What a line decides over.
pub(super) struct Target {
    /// The agent spoken to, with its lineage; `None` when no agent
    /// has that number.
    lineage: Option<Lineage>,
    /// The task the line opens.
    task: TaskId,
}

impl Act<Client> for Say {
    type Output = SayAnswer;
    type Loaded = Target;
    type Done = TaskId;

    async fn load(
        &self,
        by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Target> {
        todo!()
    }

    fn decide(
        &self,
        by: &Client,
        target: Target,
    ) -> Decision<TaskId, Self::Output> {
        todo!()
    }

    /// The line itself, which no row keeps: a repeat carries it
    /// again, and the agent takes it once by the request's id.
    fn deliveries(&self, by: &Client, task: &TaskId) -> Calls {
        todo!()
    }

    fn answer(&self, task: TaskId, made: &Made) -> Self::Output {
        todo!()
    }
}

/// The first line of `text`: a hire's purpose when it has none.
fn first_line(text: &str) -> String {
    todo!()
}
