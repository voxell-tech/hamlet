//! A chief's oversight of agents: the `grant` and `trust` tables, the
//! footing a reach is built on, and `Revoke`.

pub(super) mod asking;
pub(super) mod reserved;

use anyhow::{Result, bail};
use hamlet_messages::realm::Done;
use hamlet_messages::realm::oversight::{Revoke, RevokeRefusal};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Footing;

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

/// `agent`'s footing: its hamlet's folder and mode, the network, its
/// trust, and the secrets' version.
#[expect(dead_code, reason = "U1 builds it")]
pub(super) async fn footing(
    _rows: &Rows<'_>,
    _agent: AgentNumber,
) -> Result<Footing> {
    bail!("not built yet: U1")
}

impl Act<Client> for Revoke {
    type Output = Done<RevokeRefusal>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Client,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U1")
    }

    fn decide(
        &self,
        _by: &Client,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}
