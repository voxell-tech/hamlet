//! The `realm` actor, one per realm: the chief, the hamlets, the tree
//! of hires, the roads and letters, the tasks and who pays, the waits
//! for a chief, each agent's reach and place, and the world a client
//! reads.
//!
//! Every call it takes is an entry of `Realm`'s `Actions`, whose
//! type says who may make it (`callers.rs`). An action is an
//! `act::Act`: it loads rows, decides over them as a function of
//! values, and `act::run` commits what it decided, then answers and
//! makes its calls. Nothing else opens a transaction for an action,
//! and nothing that decides can reach another actor.

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

use self::callers::{Bare, ByHand, Looks};

/// Every table of the realm, made at each start.
const SCHEMA: &str = include_str!("realm/schema.sql");

/// The longest a line or a letter to a `hiring` hire waits for its
/// steps. A handler cannot read its caller's deadline, so the wait
/// and the call before the answer stay under the shortest one a
/// caller gives: a caller cut off first would not know the outcome.
const HIRING_WAIT: Duration = Duration::from_secs(4);

/// The shortest deadline a caller of the realm gives: an agent's
/// call, and a client's through the front door.
const SHORTEST_CALLER_DEADLINE: Duration = Duration::from_secs(10);

const _: () = assert!(
    HIRING_WAIT.as_millis() + peer::BEFORE_ANSWER.as_millis()
        < SHORTEST_CALLER_DEADLINE.as_millis(),
    "a Say waits for the hire, then submits, inside a caller's \
     deadline"
);

/// The realm's settings, given by its host as a value of
/// [`hamlet_host::HostOptions`]. For tests: the assertion above holds
/// the default, not a value given here.
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
    /// The world a client reads, as of the last commit.
    world: world::Held,
    /// The step rows being driven.
    drives: steps::Drives,
    hiring_wait: Duration,
}

#[async_trait::async_trait]
impl Actor for Realm {
    type Input = ();
    /// The table of callers. An entry's type says who makes the call
    /// and how its repeat is told: [`Asked`] and the client's own
    /// types are the chief's, under a request id; a [`Tool`] is an
    /// agent's society tool, under its call id, and a [`ByHand`] tool
    /// only the Hand holds; a [`Bare`] call is told by a row of its
    /// own or changes nothing; a [`Looks`] call reads the copy in
    /// memory. The sender of a `Bare` or a `Looks` is its message's
    /// own. An entry whose act is not written for that sender does
    /// not build.
    ///
    /// The framework serves the first entry with a name and refuses
    /// no second one, so `tests::every_name_is_listed_once` checks
    /// that no name is listed twice.
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

    /// The first start: the realm's row, the chief, profile 1 and the
    /// Hand as agent 1, `hiring`, with its standing task and its hire
    /// row. The commit reserves the row's drive and starts it.
    async fn on_create(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> Result<()> {
        change::commit(&self, &ctx, chiefs::first_start()).await?;
        Ok(())
    }

    /// Starts a drive of every step row that is not done. It reads
    /// rows and registers tasks, and waits for no call: the start has
    /// a deadline. It runs before the first call is served, so no
    /// action finds a `hiring` row with no drive.
    async fn on_start(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
        steps::drive_undone(&self, &ctx).await
    }
}

/// The error of a body a background task reaches before the unit
/// named fills it. The drive or the task logs it, and the realm runs
/// on.
fn not_built(unit: &str) -> anyhow::Error {
    anyhow::anyhow!("the scaffold: {unit} fills this")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use hamlet_actor::{ActionSet, Actor};

    use super::Realm;

    /// A name listed twice would never be served.
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
