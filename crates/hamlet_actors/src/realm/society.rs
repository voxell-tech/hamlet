//! Roads between hamlets that both sides agree to: the `agreement`
//! table and `AgreeRoad`.

use anyhow::{Result, bail};
use hamlet_messages::address::Agent;
use hamlet_messages::realm::{
    AgentRefusal, AgreeRefusal, AgreeRoad, Agreed,
};

use super::act::{Act, Decision, Unbuilt};
use super::calls::Made;
use super::change::{Affected, Rows, Writer};

#[derive(Debug, PartialEq)]
pub(super) enum Change {}

pub(super) async fn apply(
    _writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {}
}

impl Act<Agent> for AgreeRoad {
    type Output = Agreed<AgentRefusal<AgreeRefusal>>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Agent,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U11")
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}
