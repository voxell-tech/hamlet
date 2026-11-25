//! The stop of agents, one act of the realm.

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_messages::agent::chat::Stopped;
use hamlet_messages::realm::lines::StopAgents;
use hamlet_messages::sender::{Client, sender};
use hamlet_messages::workspace::{self, AgentNumber};
use serde::{Deserialize, Serialize};

use super::Realm;
use super::act::{self, Act, Decided, Decision};
use super::calls::{Calls, Delivery, Made};
use super::change::Rows;
use super::tree::{self, Life};
use super::waits::{self, Lapsing, Told};

/// The agents a stop names, once each, with the numbers of their
/// stops.
#[derive(Serialize)]
struct Stopping(Vec<(AgentNumber, u64)>);

/// The living agents a stop stopped, with the numbers of their
/// stops, and whom the lapse of their waits is told to.
#[derive(Default, Serialize, Deserialize)]
struct Halted {
    stops: Vec<(AgentNumber, u64)>,
    told: Vec<Told>,
}

impl Handler<StopAgents> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        stop: StopAgents,
    ) -> Result<Stopped> {
        let by = sender(&ctx, &stop)?;
        let mut agents = stop.agents;
        agents.sort_unstable();
        agents.dedup();
        let stopping = Stopping(
            agents
                .into_iter()
                .map(|agent| (agent, self.lines.number()))
                .collect(),
        );
        act::run(&self, &ctx, by, None, StopAgents::NAME, stopping)
            .await
    }
}

impl Act<Client> for Stopping {
    type Output = Stopped;
    type Loaded = Vec<(AgentNumber, u64, Vec<Lapsing>)>;
    type Done = Halted;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Self::Loaded> {
        let mut living = Vec::new();
        for &(agent, number) in &self.0 {
            if tree::life(rows, agent).await? == Life::Living {
                let open = waits::lapsing(rows, agent, None).await?;
                living.push((agent, number, open));
            }
        }
        Ok(living)
    }

    fn decide(
        &self,
        _by: &Client,
        living: Self::Loaded,
    ) -> Decision<Halted, Stopped> {
        let mut decided = Decided::new(Halted::default());
        for (agent, number, open) in living {
            let lapsed =
                waits::closing(workspace::Lapse::Stopped, open);
            decided.changes.extend(lapsed.changes);
            decided.done.stops.push((agent, number));
            decided.done.told.push(lapsed.done);
        }
        Decision::Change(decided)
    }

    /// Each agent's stop, before the answer and so in its lane before
    /// the lapse's deliveries, which follow the answer.
    fn deliveries(&self, _by: &Client, halted: &Halted) -> Calls {
        Calls {
            before_answer: halted
                .stops
                .iter()
                .map(|&(to, number)| Delivery::Stop { to, number })
                .collect(),
            after_answer: halted
                .told
                .iter()
                .flat_map(|told| told.calls().after_answer)
                .collect(),
        }
    }

    fn answer(&self, _halted: Halted, made: &Made) -> Stopped {
        Stopped {
            turns: made.stopped(),
        }
    }
}
