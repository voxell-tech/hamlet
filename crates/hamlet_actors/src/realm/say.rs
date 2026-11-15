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

pub(super) struct Target {
    lineage: Option<Lineage>,
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

    fn deliveries(&self, by: &Client, task: &TaskId) -> Calls {
        todo!()
    }

    fn answer(&self, task: TaskId, made: &Made) -> Self::Output {
        todo!()
    }
}

fn first_line(text: &str) -> String {
    todo!()
}
