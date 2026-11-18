//! The `realm` actor, one per realm.

mod act;
mod allowance;
mod budget;
mod callers;
mod calls;
mod change;
mod chiefs;
mod documents;
mod hire;
mod laid;
mod letters;
mod marker;
mod oversight;
mod parcels;
mod peer;
mod place;
mod reach;
mod record;
mod reviews;
mod roads;
mod say;
mod secrets;
mod settings;
mod society;
mod steps;
mod tasks;
mod tree;
mod versions;
mod waits;
mod workers;
mod world;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hamlet_actor::{Actor, Ctx, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory};
use hamlet_host::{Caller, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::realm::budget::{
    Charge, MaySpend, OpenTask, ReadBudget, ResetBudget,
};
use hamlet_messages::realm::choices::{SetModel, SetNickname};
use hamlet_messages::realm::client::{Answer, ReadWorld, Say};
use hamlet_messages::realm::documents::{
    EditCatalogue, EditCharter, EditSettings, ReadCharter, ReadCopies,
};
use hamlet_messages::realm::hamlets::{
    MoveHamlet, NameHamlet, ReadRecord, SetFolder, SetMode,
};
use hamlet_messages::realm::models::{
    Credential, KeepCredential, TurnFailed,
};
use hamlet_messages::realm::oversight::{
    AskChief, Lapse, Object, Revoke,
};
use hamlet_messages::realm::parcels::FetchPart;
use hamlet_messages::realm::workers::{
    ErrandDone, SetAllowance, StartWorker, WorkerDone,
};
use hamlet_messages::realm::{
    AgreeRoad, Ask, Asked, End, Found, Hire, LayRoad, NeedsChief,
    Reply, RequestRoad, Seat, Secrets, SetLimits, SetTier, SetTools,
    TakeUpRoad, Tell, Tool,
};

use self::callers::{Bare, ByHand, Looks};

const SCHEMA: &str = include_str!("realm/schema.sql");

const HIRING_WAIT: Duration = Duration::from_secs(4);

const REVIEW_DEADLINE: Duration = Duration::from_secs(120);

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
    /// The longest a line held for a lead's review waits for it.
    pub review_deadline: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            hiring_wait: HIRING_WAIT,
            review_deadline: REVIEW_DEADLINE,
        }
    }
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
    secrets: secrets::Held,
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
        Asked<Revoke>,
        Bare<TurnFailed>,
        Bare<Lapse>,
        Tool<AskChief>,
        Tool<Object>,
        Bare<ErrandDone>,
        Tool<StartWorker>,
        Asked<StartWorker>,
        Bare<WorkerDone>,
        Tool<SetAllowance>,
        Asked<SetAllowance>,
        Tool<ReadBudget>,
        OpenTask,
        Asked<NameHamlet>,
        Asked<MoveHamlet>,
        Asked<SetMode>,
        Asked<SetFolder>,
        Tool<SetNickname>,
        Asked<SetNickname>,
        Tool<SetModel>,
        Asked<SetModel>,
        ByHand<Tool<ReadRecord>>,
        Asked<ReadRecord>,
        Asked<EditCharter>,
        Asked<EditSettings>,
        Asked<EditCatalogue>,
        Bare<ReadCharter>,
        Bare<ReadCopies>,
        Tool<AgreeRoad>,
        Looks<Credential>,
        KeepCredential,
        FetchPart,
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
            secrets: secrets::Held::load(host.folder())?,
            peers: peer::Peers::new(host),
            world: world::Held::load(ctx).await?,
            drives: steps::Drives::default(),
        })
    }

    async fn on_create(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> Result<()> {
        let rows = change::Rows::outside(&ctx)?;
        let changes = chiefs::first_start(&rows).await?;
        change::commit(&self, &ctx, changes).await?;
        Ok(())
    }

    async fn on_start(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
        parcels::sweep(&self, &ctx).await?;
        let rows = change::Rows::outside(&ctx)?;
        steps::undone(&rows, &self.drives).await?.start(&self, &ctx);
        Ok(())
    }
}

// The crate's one testing module, in the realm, since moving it would
// open the realm's private modules to the crate.
#[cfg(test)]
mod testing {
    use std::sync::Arc;

    use hamlet_actor::{Actor as _, Ctx, Registry, run_actor};
    use hamlet_actor_core::ActorConfig;
    use hamlet_host::{Host, HostOptions};
    use hamlet_messages::address::{self, Party as _};
    use hamlet_messages::realm::{HamletId, Tier};
    use hamlet_messages::workspace::AgentNumber;
    use hamlet_testing::Scratch;
    use tokio::sync::mpsc;

    use super::change::{self, Rows};
    use super::{Realm, chiefs, tree};

    pub(super) struct Started {
        pub realm: Arc<Realm>,
        pub ctx: Ctx<Realm>,
        host: Host,
        _scratch: Scratch,
    }

    impl Started {
        pub async fn new() -> Self {
            let (sent, mut started) = mpsc::unbounded_channel();
            let mut registry = Registry::new();
            #[expect(
                deprecated,
                reason = "the one registration that hands a test \
                          the realm's context"
            )]
            registry.register_with::<Realm, _, _>(
                address::Realm::KIND,
                ActorConfig {
                    has_database: true,
                    ..ActorConfig::default()
                },
                move |start| {
                    let _ = sent.send(start.ctx.clone());
                    run_actor(start)
                },
            );
            let scratch = Scratch::new("realm-module");
            let host = Host::open_with(
                &scratch,
                registry.into_factories(),
                HostOptions::default(),
            )
            .unwrap();
            host.get_or_start(&address::Realm.address().unwrap())
                .await
                .unwrap();
            let ctx = started.recv().await.unwrap();
            Self {
                realm: Arc::new(Realm::create(&ctx).await.unwrap()),
                ctx,
                host,
                _scratch: scratch,
            }
        }

        pub async fn stop(self) {
            self.host.shutdown().await.unwrap();
        }

        pub fn rows(&self) -> Rows<'_> {
            Rows::outside(&self.ctx).unwrap()
        }

        pub async fn commit(
            &self,
            made: impl Into<change::Change>,
        ) -> anyhow::Result<()> {
            change::commit(&self.realm, &self.ctx, vec![made.into()])
                .await?;
            Ok(())
        }

        pub async fn race<T, R>(
            &self,
            read: impl FnOnce(Ctx<Realm>) -> R,
            writes: &[&str],
        ) -> T
        where
            R: Future<Output = anyhow::Result<T>> + Send + 'static,
            T: Send + 'static,
        {
            let read = tokio::spawn(read(self.ctx.clone()));
            let tx =
                self.ctx.sql().begin_transaction(None).await.unwrap();
            for &write in writes {
                tx.execute(write, None).await.unwrap();
            }
            tx.commit().await.unwrap();
            read.await.unwrap().unwrap()
        }

        pub async fn found(
            &self,
            hamlet: &HamletId,
            mayor: AgentNumber,
        ) {
            self.commit(chiefs::Change::Found {
                hamlet: hamlet.clone(),
                name: hamlet.0.clone(),
                mayor,
                tier: Tier::Fifty,
            })
            .await
            .unwrap();
            self.hire(hamlet, mayor, None).await;
        }

        pub async fn hire(
            &self,
            hamlet: &HamletId,
            agent: AgentNumber,
            lead: Option<AgentNumber>,
        ) {
            self.commit(tree::Change::Hire {
                agent,
                hamlet: hamlet.clone(),
                lead,
                of: tree::Of::Profile(tree::HAND_PROFILE),
                purpose: None,
            })
            .await
            .unwrap();
        }
    }
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
