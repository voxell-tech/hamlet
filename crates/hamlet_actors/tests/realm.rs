//! The realm through a host, called as a client and as agents, with
//! stand-ins for the actors it calls. Each unit's checks are in a
//! file of their own below.

#![expect(
    dead_code,
    reason = "the scaffold: the units' tests call these"
)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory};
use hamlet_actor_error::ActorError;
use hamlet_actors::realm;
use hamlet_host::{Address, Caller, Host, HostOptions, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::{AgentInput, Fill, Place, Submit};
use hamlet_messages::profile::Branch;
use hamlet_messages::realm::ProfileId;
use hamlet_messages::workspace::reach::Reach;
use hamlet_messages::workspace::{
    AgentNumber, AnswerTaken, ChiefAnswer, InForce, ReachVersion,
    SetReach,
};
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[path = "realm/budget.rs"]
mod budget;
#[path = "realm/first_start.rs"]
mod first_start;
#[path = "realm/found.rs"]
mod found;
#[path = "realm/laid.rs"]
mod laid;
#[path = "realm/lead.rs"]
mod lead;
#[path = "realm/letters.rs"]
mod letters;
#[path = "realm/places.rs"]
mod places;
#[path = "realm/say.rs"]
mod say;
#[path = "realm/steps.rs"]
mod steps;
#[path = "realm/waits.rs"]
mod waits;
#[path = "realm/world.rs"]
mod world;
#[path = "realm/writers.rs"]
mod writers;

/// How long a test waits for a call, or for a hire to go live.
const DEADLINE: Duration = Duration::from_secs(20);

/// What the stand-ins were called with, in order, and how they
/// answer. A test sets the answers before [`Realm::start_with`].
#[derive(Default)]
struct Seen {
    /// Each `agent/<n>` made, with its input.
    made: Mutex<Vec<(AgentNumber, AgentInput)>>,
    /// Each `Submit`, by its agent.
    submits: Mutex<Vec<(AgentNumber, Submit)>>,
    /// Each `Place`, by its agent.
    places: Mutex<Vec<(AgentNumber, Place)>>,
    /// Each `Fill`, by its agent.
    fills: Mutex<Vec<(AgentNumber, Fill)>>,
    /// Each `SetReach` the workspace stand-in kept.
    reaches: Mutex<Vec<SetReach>>,
    /// Each `ChiefAnswer` the workspace stand-in got.
    chief_answers: Mutex<Vec<ChiefAnswer>>,
    /// Each `Branch`, by its profile.
    branches: Mutex<Vec<(ProfileId, Branch)>>,
    /// How long a profile takes to answer a `Branch`: a hire then
    /// stays `hiring` that long.
    branch_delay: Duration,
    /// The callees that fail once each, by the name of what they are
    /// called with: `branch`, `create`, `fill`, `set_reach`.
    fail_once: Mutex<HashSet<&'static str>>,
}

impl Seen {
    /// Fails `call` when `fail_once` lists it, and takes it off the
    /// list.
    fn fails(&self, call: &'static str) -> anyhow::Result<()> {
        if self.fail_once.lock().unwrap().remove(call) {
            bail!("the stand-in fails `{call}` once");
        }
        Ok(())
    }
}

/// Calls `name` with `args` at the realm, as the stand-in asked to:
/// the test's way to call the realm as an agent or as the workspace.
#[derive(Serialize, Deserialize)]
struct Forward {
    name: String,
    args: Vec<u8>,
}

impl Action for Forward {
    type Output = Vec<u8>;
    const NAME: &'static str = "forward";
}

async fn forward<A: Actor>(
    ctx: &Ctx<A>,
    forward: Forward,
) -> anyhow::Result<Vec<u8>> {
    HostRef::of(ctx.inner())?
        .call(
            &address::Realm.address()?,
            &forward.name,
            forward.args,
            DEADLINE,
        )
        .await
}

fn seen<A: Actor>(ctx: &Ctx<A>) -> anyhow::Result<Arc<Seen>> {
    HostRef::of(ctx.inner())?
        .value::<Seen>()
        .context("the host holds what the stand-ins saw")
}

/// Stands in for `agent/<n>`: records what it is made and called
/// with, and calls the realm as itself when a test asks it to.
struct AgentStandIn {
    seen: Arc<Seen>,
    agent: AgentNumber,
}

#[async_trait::async_trait]
impl Actor for AgentStandIn {
    /// `None` for an agent a test made to call as.
    type Input = Option<AgentInput>;
    type Actions = (Submit, Place, Fill, Forward);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self {
            seen: seen(ctx)?,
            agent: AgentNumber(ctx.actor_id().parse()?),
        })
    }

    async fn on_create(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> anyhow::Result<()> {
        self.seen.fails("create")?;
        if let Some(input) = ctx.input()? {
            self.seen.made.lock().unwrap().push((self.agent, input));
        }
        Ok(())
    }
}

impl Handler<Submit> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        submit: Submit,
    ) -> anyhow::Result<()> {
        self.seen.submits.lock().unwrap().push((self.agent, submit));
        Ok(())
    }
}

impl Handler<Place> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        place: Place,
    ) -> anyhow::Result<()> {
        self.seen.places.lock().unwrap().push((self.agent, place));
        Ok(())
    }
}

impl Handler<Fill> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        fill: Fill,
    ) -> anyhow::Result<()> {
        self.seen.fails("fill")?;
        self.seen.fills.lock().unwrap().push((self.agent, fill));
        Ok(())
    }
}

impl Handler<Forward> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Forward,
    ) -> anyhow::Result<Vec<u8>> {
        forward(&ctx, asked).await
    }
}

/// Stands in for `profile/<id>`: answers a `Branch` after
/// `branch_delay`, the same for a branch it made before.
struct ProfileStandIn {
    seen: Arc<Seen>,
    profile: ProfileId,
}

#[async_trait::async_trait]
impl Actor for ProfileStandIn {
    type Input = ();
    type Actions = (Branch,);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self {
            seen: seen(ctx)?,
            profile: ProfileId(ctx.actor_id().parse()?),
        })
    }
}

impl Handler<Branch> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        branch: Branch,
    ) -> anyhow::Result<()> {
        tokio::time::sleep(self.seen.branch_delay).await;
        self.seen.fails("branch")?;
        self.seen
            .branches
            .lock()
            .unwrap()
            .push((self.profile, branch));
        Ok(())
    }
}

/// Stands in for `workspace/local`, started before the realm: keeps a
/// reach only when it is newer than the agent's last, and calls the
/// realm as the workspace when a test asks it to.
struct WorkspaceStandIn {
    seen: Arc<Seen>,
}

#[async_trait::async_trait]
impl Actor for WorkspaceStandIn {
    type Input = ();
    type Actions = (SetReach, ChiefAnswer, Forward);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self { seen: seen(ctx)? })
    }
}

impl Handler<SetReach> for WorkspaceStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        set: SetReach,
    ) -> anyhow::Result<InForce> {
        self.seen.fails("set_reach")?;
        let mut kept = self.seen.reaches.lock().unwrap();
        let in_force = kept
            .iter()
            .filter(|reach| reach.agent == set.agent)
            .map(|reach| reach.version)
            .max();
        if let Some(version) = in_force
            && version >= set.version
        {
            return Ok(InForce { version });
        }
        let version = set.version;
        kept.push(set);
        Ok(InForce { version })
    }
}

impl Handler<ChiefAnswer> for WorkspaceStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        answer: ChiefAnswer,
    ) -> anyhow::Result<AnswerTaken> {
        self.seen.chief_answers.lock().unwrap().push(answer);
        Ok(AnswerTaken::Taken)
    }
}

impl Handler<Forward> for WorkspaceStandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Forward,
    ) -> anyhow::Result<Vec<u8>> {
        forward(&ctx, asked).await
    }
}

fn kind<A: Actor>(name: &str) -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<A>(name, ActorConfig::default());
    registry
        .into_factories()
        .remove(name)
        .expect("the registry holds the kind it registered")
}

/// A host on `dir` with the realm and the stand-ins, the workspace's
/// started.
async fn open(
    dir: &Path,
    seen: &Arc<Seen>,
    options: Option<&Arc<realm::Options>>,
) -> Host {
    let kinds = HashMap::from([
        (address::Realm::KIND.to_owned(), realm::factory()),
        (
            address::Agent::KIND.to_owned(),
            kind::<AgentStandIn>(address::Agent::KIND),
        ),
        (
            address::Profile::KIND.to_owned(),
            kind::<ProfileStandIn>(address::Profile::KIND),
        ),
        (
            address::Workspace::KIND.to_owned(),
            kind::<WorkspaceStandIn>(address::Workspace::KIND),
        ),
    ]);
    let mut host_options =
        HostOptions::default().with_value(seen.clone());
    if let Some(options) = options {
        host_options = host_options.with_value(options.clone());
    }
    let host = Host::open_with(dir, kinds, host_options).unwrap();
    host.get_or_start(&address::Workspace.address().unwrap())
        .await
        .unwrap();
    host
}

/// A host with the realm and the three stand-ins, on a scratch
/// folder.
struct Realm {
    host: Host,
    seen: Arc<Seen>,
    options: Option<Arc<realm::Options>>,
    scratch: Scratch,
}

impl Realm {
    /// The realm with its own settings, and stand-ins that answer at
    /// once.
    async fn start() -> Self {
        Self::start_with(None, Seen::default()).await
    }

    /// The realm with `options`, and stand-ins that answer as `seen`
    /// says.
    async fn start_with(
        options: Option<realm::Options>,
        seen: Seen,
    ) -> Self {
        let scratch = Scratch::new("realm");
        let seen = Arc::new(seen);
        let options = options.map(Arc::new);
        let host = open(&scratch, &seen, options.as_ref()).await;
        host.get_or_start(&address::Realm.address().unwrap())
            .await
            .unwrap();
        Self {
            host,
            seen,
            options,
            scratch,
        }
    }

    /// Stops the host and starts it again on the same folder: the
    /// realm's start.
    async fn restart(self) -> Self {
        self.host.shutdown().await.unwrap();
        let host =
            open(&self.scratch, &self.seen, self.options.as_ref())
                .await;
        host.get_or_start(&address::Realm.address().unwrap())
            .await
            .unwrap();
        Self { host, ..self }
    }

    /// Calls the realm as the client.
    async fn ask<A: Action>(
        &self,
        action: &A,
    ) -> anyhow::Result<A::Output> {
        let args = hamlet_actor::action::encode_positional(action)?;
        let answer = self
            .host
            .call_for_client(
                &address::Realm.address()?,
                A::NAME,
                args,
                DEADLINE,
            )
            .await?;
        decoded(&answer)
    }

    /// Calls the realm as `agent/<n>`, through its stand-in, made
    /// first when the realm has not made it.
    async fn tool<A: Action>(
        &self,
        as_agent: AgentNumber,
        action: &A,
    ) -> anyhow::Result<A::Output> {
        self.call_as(&address::Agent(as_agent).address()?, action)
            .await
    }

    /// Calls the realm as the workspace, through its stand-in.
    async fn call_as_workspace<A: Action>(
        &self,
        action: &A,
    ) -> anyhow::Result<A::Output> {
        self.call_as(&address::Workspace.address()?, action).await
    }

    async fn call_as<A: Action>(
        &self,
        from: &Address,
        action: &A,
    ) -> anyhow::Result<A::Output> {
        self.host.get_or_start(from).await?;
        let forward = Forward {
            name: A::NAME.to_owned(),
            args: hamlet_actor::action::encode_positional(action)?,
        };
        let reply = self
            .host
            .call(
                from,
                Forward::NAME,
                hamlet_actor::action::encode_positional(&forward)?,
                DEADLINE,
            )
            .await?;
        decoded(&decoded::<Vec<u8>>(&reply)?)
    }

    /// Waits until `agent`'s hire row has every step done.
    async fn live(&self, agent: AgentNumber) {
        let begun = Instant::now();
        let n = i64::from(agent.0);
        loop {
            let done = self
                .rows()
                .query_row(
                    "SELECT done FROM step WHERE kind = 'hire' AND \
                     n = ?1",
                    [n],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()
                .unwrap()
                .flatten();
            if done.as_deref() == Some("live") {
                return;
            }
            assert!(
                begun.elapsed() < DEADLINE,
                "agent {} is not live: its last step done is \
                 {done:?}",
                agent.0
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// The realm's file, for what no answer shows.
    fn rows(&self) -> Connection {
        Connection::open_with_flags(
            self.file(),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .unwrap()
    }

    fn file(&self) -> PathBuf {
        self.scratch.join("actors/realm/local.sqlite")
    }
}

fn decoded<T: DeserializeOwned>(bytes: &[u8]) -> anyhow::Result<T> {
    ciborium::from_reader(bytes).context("decode the answer")
}

/// The `group.code` of the error a call failed with.
fn error_code<T: std::fmt::Debug>(
    called: anyhow::Result<T>,
) -> String {
    let error = ActorError::extract(&called.unwrap_err());
    format!("{}.{}", error.group(), error.code())
}

/// The workspace's stand-in keeps only a newer reach, and fails a
/// call `fail_once` lists once, with no realm.
#[tokio::test(flavor = "multi_thread")]
async fn the_workspace_stand_in_keeps_only_a_newer_reach() {
    let scratch = Scratch::new("realm-stand-ins");
    let seen = Arc::new(Seen::default());
    seen.fail_once.lock().unwrap().insert("set_reach");
    let host = open(&scratch, &seen, None).await;
    let workspace = address::Workspace.address().unwrap();
    let set = async |version| {
        let set = SetReach {
            agent: AgentNumber(2),
            version: ReachVersion(version),
            reach: Reach::Ended,
        };
        let args = hamlet_actor::action::encode_positional(&set)?;
        let answer = host
            .call(&workspace, SetReach::NAME, args, DEADLINE)
            .await?;
        decoded::<InForce>(&answer)
    };
    assert!(set(2).await.is_err(), "it fails `set_reach` once");
    for (version, in_force) in [(2, 2), (1, 2), (3, 3)] {
        assert_eq!(
            set(version).await.unwrap().version,
            ReachVersion(in_force)
        );
    }
    let kept = seen
        .reaches
        .lock()
        .unwrap()
        .iter()
        .map(|reach| reach.version)
        .collect::<Vec<_>>();
    assert_eq!(kept, [ReachVersion(2), ReachVersion(3)]);
    host.shutdown().await.unwrap();
}
