//! The `realm` actor, one per realm.

mod act;
mod budget;
mod callers;
mod calls;
mod change;
mod chiefs;
mod hire;
mod laid;
mod letters;
mod marker;
mod peer;
mod place;
mod reach;
mod record;
mod roads;
mod say;
mod secrets;
mod steps;
mod tasks;
mod tree;
mod versions;
mod waits;
mod world;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hamlet_actor::{Actor, Ctx, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory};
use hamlet_host::{Caller, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::realm::budget::{Charge, MaySpend, ResetBudget};
use hamlet_messages::realm::client::{Answer, ReadWorld, Say};
use hamlet_messages::realm::{
    Ask, Asked, End, Found, Hire, LayRoad, NeedsChief, Reply,
    RequestRoad, Seat, Secrets, SetLimits, SetTier, SetTools,
    TakeUpRoad, Tell, Tool,
};
use serde::Serialize;

use self::callers::{Bare, ByHand, Looks};

const SCHEMA: &str = include_str!("realm/schema.sql");

const HIRING_WAIT: Duration = Duration::from_secs(4);

const SHORTEST_CALLER_DEADLINE: Duration = Duration::from_secs(10);

const _: () = assert!(
    HIRING_WAIT.as_millis() + peer::BEFORE_ANSWER.as_millis()
        < SHORTEST_CALLER_DEADLINE.as_millis(),
    "a Say waits for the hire, then submits, inside a caller's \
     deadline"
);

/// The realm's settings, given by its host as a value of
/// [`hamlet_host::HostOptions`].
pub struct Options {
    /// The longest a line or a letter to a `hiring` hire waits for
    /// its steps.
    pub hiring_wait: Duration,
}

/// The realm's kind.
pub fn factory() -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<Realm>(
        address::Realm::KIND,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry
        .into_factories()
        .remove(address::Realm::KIND)
        .expect("the registry holds the kind it registered")
}

struct Realm {
    peers: peer::Peers,
    world: world::Held,
    drives: steps::Drives,
    hiring_wait: Duration,
}

#[async_trait::async_trait]
impl Actor for Realm {
    type Input = ();
    type Actions = (
        Asked<Found>,
        ByHand<Tool<Found>>,
        Asked<Hire>,
        Tool<Hire>,
        Asked<End>,
        Tool<End>,
        Asked<Seat>,
        ByHand<Tool<Seat>>,
        Say,
        Tool<Tell>,
        Tool<Ask>,
        Bare<Reply>,
        Bare<MaySpend>,
        Bare<Charge>,
        Asked<ResetBudget>,
        Bare<NeedsChief>,
        Answer,
        Looks<Secrets>,
        Looks<ReadWorld>,
        Asked<LayRoad>,
        Tool<LayRoad>,
        Asked<TakeUpRoad>,
        Tool<TakeUpRoad>,
        Tool<RequestRoad>,
        Asked<SetTools>,
        Tool<SetTools>,
        Asked<SetTier>,
        Asked<SetLimits>,
    );
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        let host = HostRef::of(ctx.inner())?;
        ctx.sql().exec(SCHEMA).await?;
        Ok(Self {
            hiring_wait: host
                .value::<Options>()
                .map_or(HIRING_WAIT, |options| options.hiring_wait),
            peers: peer::Peers::new(host),
            world: world::Held::load(ctx).await?,
            drives: steps::Drives::default(),
        })
    }

    async fn on_create(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> Result<()> {
        change::commit(&self, &ctx, chiefs::first_start()).await?;
        Ok(())
    }

    async fn on_start(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
        steps::drive_undone(&self, &ctx).await
    }
}

fn not_built(unit: &str) -> anyhow::Error {
    anyhow::anyhow!("the scaffold: {unit} fills this")
}

fn cbor(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use hamlet_actor::{ActionSet, Actor};

    use super::Realm;

    #[test]
    fn every_name_is_listed_once() {
        let names =
            <<Realm as Actor>::Actions as ActionSet<Realm>>::entries(
            )
            .iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        let unique = names.iter().copied().collect::<HashSet<_>>();
        assert_eq!(
            unique.len(),
            names.len(),
            "a name is listed twice: {names:?}"
        );
    }
}
