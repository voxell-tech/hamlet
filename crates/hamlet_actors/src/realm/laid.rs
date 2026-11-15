//! Roads a lead lays between two hires: the `road` table, `LayRoad`,
//! `TakeUpRoad` and `RequestRoad` (H3).

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U11 fills this file"
)]

use anyhow::Result;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::{
    AgentRefusal, Done, HamletId, LayRefusal, LayRoad, RequestRoad,
    RequestRoadRefusal, TakeUpRefusal, TakeUpRoad,
};
use hamlet_messages::workspace::AgentNumber;

use super::act::{Act, Decision};
use super::callers::Lead;
use super::calls::{Calls, Made};
use super::change::{Affected, Rows, Writer};
use super::roads;

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    Lay {
        a: AgentNumber,
        b: AgentNumber,
        hamlet: HamletId,
    },
    TakeUp {
        a: AgentNumber,
        b: AgentNumber,
    },
    Used {
        between: (AgentNumber, AgentNumber),
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Lay { a, b, hamlet } => {
            lay(writer, a, b, hamlet).await
        }
        Change::TakeUp { a, b } => take_up(writer, a, b).await,
        Change::Used { between } => used(writer, between).await,
    }
}

async fn lay(
    writer: &Writer<'_>,
    a: AgentNumber,
    b: AgentNumber,
    hamlet: HamletId,
) -> Result<Affected> {
    todo!()
}

async fn take_up(
    writer: &Writer<'_>,
    a: AgentNumber,
    b: AgentNumber,
) -> Result<Affected> {
    todo!()
}

async fn used(
    writer: &Writer<'_>,
    between: (AgentNumber, AgentNumber),
) -> Result<Affected> {
    todo!()
}

/// Whether an open laid road joins `a` and `b`.
pub(super) async fn between(
    rows: &Rows<'_>,
    a: AgentNumber,
    b: AgentNumber,
) -> Result<bool> {
    Ok(false)
}

/// The other ends of `agent`'s open laid roads.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<AgentNumber>> {
    Ok(Vec::new())
}

pub(super) struct Laying {
    ends: Option<(super::tree::Lineage, super::tree::Lineage)>,
    laid: bool,
}

impl<By: Lead> Act<By> for LayRoad {
    type Output = Done<By::Why<LayRefusal>>;
    type Loaded = Laying;
    type Done = ();

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Laying> {
        todo!()
    }

    fn decide(
        &self,
        by: &By,
        laying: Laying,
    ) -> Decision<(), Self::Output> {
        todo!()
    }

    fn answer(&self, (): (), made: &Made) -> Self::Output {
        todo!()
    }
}

impl<By: Lead> Act<By> for TakeUpRoad {
    type Output = Done<By::Why<TakeUpRefusal>>;
    type Loaded = Laying;
    type Done = ();

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Laying> {
        todo!()
    }

    fn decide(
        &self,
        by: &By,
        laying: Laying,
    ) -> Decision<(), Self::Output> {
        todo!()
    }

    fn answer(&self, (): (), made: &Made) -> Self::Output {
        todo!()
    }
}

impl Act<Agent> for RequestRoad {
    type Output = Done<AgentRefusal<RequestRoadRefusal>>;
    type Loaded = roads::Ends;
    type Done = hamlet_messages::realm::LetterId;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<roads::Ends> {
        todo!()
    }

    fn decide(
        &self,
        by: &Agent,
        ends: roads::Ends,
    ) -> Decision<Self::Done, Self::Output> {
        todo!()
    }

    fn deliveries(&self, by: &Agent, letter: &Self::Done) -> Calls {
        todo!()
    }

    fn answer(
        &self,
        letter: Self::Done,
        made: &Made,
    ) -> Self::Output {
        todo!()
    }
}
