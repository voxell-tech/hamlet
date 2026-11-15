//! The realm through a host, called as a client and as agents, with
//! stand-ins for the actors it calls.

#![expect(
    dead_code,
    reason = "the scaffold: the tests of U2 to U11 call these, and \
              the integrator removes this"
)]

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory, ShutdownKind};
use hamlet_actor_error::ActorError;
use hamlet_actors::realm;
use hamlet_host::{Address, Caller, Host, HostOptions, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::{
    AgentInput, Fill, Place, PlaceVersion, Submit,
};
use hamlet_messages::profile::Branch;
use hamlet_messages::realm::{Found, ProfileId, Tier};
use hamlet_messages::workspace::reach::Reach;
use hamlet_messages::workspace::{
    AgentNumber, AnswerTaken, ChiefAnswer, InForce, ReachVersion,
    SetReach,
};
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

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

const DEADLINE: Duration = Duration::from_secs(20);

/// A call of the realm's that a test can hold or fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Callee {
    Branch,
    Create,
    Fill,
    SetReach,
}

#[derive(Default)]
struct Seen {
    made: Mutex<Vec<(AgentNumber, AgentInput)>>,
    submits: Mutex<Vec<(AgentNumber, Submit)>>,
    places: Mutex<Vec<(AgentNumber, Place)>>,
    fills: Mutex<Vec<(AgentNumber, Fill)>>,
    reaches: Mutex<Vec<SetReach>>,
    chief_answers: Mutex<Vec<ChiefAnswer>>,
    branches: Mutex<Vec<(ProfileId, Branch)>>,
    fail_once: Mutex<HashSet<Callee>>,
    held: watch::Sender<HashSet<Callee>>,
}

impl Seen {
    /// Waits while the test holds `callee`, then fails it if the
    /// test asked it to fail once.
    async fn called(&self, callee: Callee) -> anyhow::Result<()> {
        self.held
            .subscribe()
            .wait_for(|held| !held.contains(&callee))
            .await?;
        if self.fail_once.lock().unwrap().remove(&callee) {
            bail!("the stand-in fails {callee:?} once");
        }
        Ok(())
    }

    fn hold(&self, callee: Callee) {
        self.held.send_modify(|held| {
            held.insert(callee);
        });
    }

    fn release(&self, callee: Callee) {
        self.held.send_modify(|held| {
            held.remove(&callee);
        });
    }
}

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

/// The key of the stand-in's address, `kind/key`, as a number.
fn key<A: Actor, N: FromStr>(ctx: &Ctx<A>) -> anyhow::Result<N>
where
    N::Err: std::error::Error + Send + Sync + 'static,
{
    let id = ctx.actor_id();
    let (_, key) =
        id.split_once('/').context("an actor id is kind/key")?;
    Ok(key.parse()?)
}

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
            agent: AgentNumber(key(ctx)?),
        })
    }

    async fn on_create(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> anyhow::Result<()> {
        self.seen.called(Callee::Create).await?;
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
        self.seen.called(Callee::Fill).await?;
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
            profile: ProfileId(key(ctx)?),
        })
    }
}

impl Handler<Branch> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        branch: Branch,
    ) -> anyhow::Result<()> {
        self.seen.called(Callee::Branch).await?;
        self.seen
            .branches
            .lock()
            .unwrap()
            .push((self.profile, branch));
        Ok(())
    }
}

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
        self.seen.called(Callee::SetReach).await?;
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

struct Realm {
    host: Host,
    seen: Arc<Seen>,
    options: Option<Arc<realm::Options>>,
    scratch: Scratch,
}

impl Realm {
    async fn start() -> Self {
        Self::start_with(None, Seen::default()).await
    }

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

    async fn tool<A: Action>(
        &self,
        as_agent: AgentNumber,
        action: &A,
    ) -> anyhow::Result<A::Output> {
        self.call_as(&address::Agent(as_agent), action).await
    }

    async fn call_as_workspace<A: Action>(
        &self,
        action: &A,
    ) -> anyhow::Result<A::Output> {
        self.call_as(&address::Workspace, action).await
    }

    async fn call_as<A: Action>(
        &self,
        from: &impl Party,
        action: &A,
    ) -> anyhow::Result<A::Output> {
        let forward = Forward {
            name: A::NAME.to_owned(),
            args: hamlet_actor::action::encode_positional(action)?,
        };
        decoded(&call(&self.host, from, &forward).await?)
    }

    /// Waits for `agent`'s hire to be live.
    async fn live(&self, agent: AgentNumber) {
        // The reads below can still repeat `SetReach`: a stop before
        // the reach step commits runs that step again.
        self.reach_held(agent).await;
        let begun = Instant::now();
        let n = i64::from(agent.0);
        loop {
            let done = self
                .rows(|db| {
                    db.query_row(
                        "SELECT done FROM step WHERE kind = 'hire' \
                         AND n = ?1",
                        [n],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()
                })
                .await
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

    /// Waits in memory for the workspace to hold `agent`'s reach, its
    /// hire's last call. A stopped read before it starts the realm
    /// again, which would do the rest of the steps itself.
    async fn reach_held(&self, agent: AgentNumber) {
        until(
            &format!("the workspace holds agent {}'s reach", agent.0),
            || self.holds_reach(agent),
        )
        .await;
    }

    fn holds_reach(&self, agent: AgentNumber) -> bool {
        self.seen
            .reaches
            .lock()
            .unwrap()
            .iter()
            .any(|reach| reach.agent == agent)
    }

    async fn rows<T>(
        &self,
        read: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> T {
        let realm = address::Realm.address().unwrap();
        stopped_open(
            &self.host,
            &self.scratch,
            &realm,
            OpenFlags::SQLITE_OPEN_READ_ONLY,
            read,
        )
        .await
        .unwrap()
    }

    /// Runs `sql` on the realm's file, which starts the realm again
    /// as a stopped read does.
    async fn write(&self, sql: &str) {
        let realm = address::Realm.address().unwrap();
        stopped_open(
            &self.host,
            &self.scratch,
            &realm,
            OpenFlags::default(),
            |db| db.execute_batch(sql),
        )
        .await
        .unwrap();
    }
}

/// Opens the file of the actor at `at` with `flags`. The host holds
/// the file locked while the actor runs, so this stops the actor and
/// starts it again.
async fn stopped_open<T>(
    host: &Host,
    dir: &Path,
    at: &Address,
    flags: OpenFlags,
    open: impl FnOnce(&Connection) -> rusqlite::Result<T>,
) -> anyhow::Result<T> {
    host.stop(at, ShutdownKind::Sleep).await?;
    let file = dir
        .join("actors")
        .join(at.kind())
        .join(format!("{}.sqlite", at.key()));
    let opened = open(&Connection::open_with_flags(file, flags)?);
    host.get_or_start(at).await?;
    Ok(opened?)
}

async fn call<A: Action>(
    host: &Host,
    to: &impl Party,
    action: &A,
) -> anyhow::Result<A::Output> {
    let to = to.address()?;
    host.get_or_start(&to).await?;
    let args = hamlet_actor::action::encode_positional(action)?;
    decoded(&host.call(&to, A::NAME, args, DEADLINE).await?)
}

async fn until(what: &str, holds: impl Fn() -> bool) {
    let begun = Instant::now();
    while !holds() {
        assert!(begun.elapsed() < DEADLINE, "never: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn found(name: &str) -> Found {
    Found {
        name: name.to_owned(),
        mayor: None,
        tier: Tier::Fifty,
        purpose: None,
    }
}

fn decoded<T: DeserializeOwned>(bytes: &[u8]) -> anyhow::Result<T> {
    ciborium::from_reader(bytes).context("decode the answer")
}

fn error_code<T: std::fmt::Debug>(
    called: anyhow::Result<T>,
) -> String {
    let error = ActorError::extract(&called.unwrap_err());
    format!("{}.{}", error.group(), error.code())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_workspace_stand_in_keeps_only_a_newer_reach() {
    let scratch = Scratch::new("realm-stand-ins");
    let seen = Arc::new(Seen::default());
    seen.fail_once.lock().unwrap().insert(Callee::SetReach);
    let host = open(&scratch, &seen, None).await;
    let set = async |version| {
        let set = SetReach {
            agent: AgentNumber(2),
            version: ReachVersion(version),
            reach: Reach::Ended,
        };
        call(&host, &address::Workspace, &set).await
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

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_and_profile_stand_ins_take_their_calls() {
    let scratch = Scratch::new("realm-stand-ins");
    let seen = Arc::new(Seen::default());
    let host = open(&scratch, &seen, None).await;
    let place = Place {
        version: PlaceVersion(1),
        lead: None,
        hires: Vec::new(),
        roads: Vec::new(),
        tools_off: Vec::new(),
    };
    let branch = Branch {
        name: "hires/2".into(),
    };

    call(&host, &address::Agent(AgentNumber(2)), &place)
        .await
        .unwrap();
    call(&host, &address::Profile(ProfileId(1)), &branch)
        .await
        .unwrap();

    assert_eq!(
        *seen.places.lock().unwrap(),
        [(AgentNumber(2), place)]
    );
    assert_eq!(
        *seen.branches.lock().unwrap(),
        [(ProfileId(1), branch)]
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_reach_answers_once_released() {
    let scratch = Scratch::new("realm-stand-ins");
    let seen = Arc::new(Seen::default());
    let host = open(&scratch, &seen, None).await;
    let set = SetReach {
        agent: AgentNumber(2),
        version: ReachVersion(1),
        reach: Reach::Ended,
    };
    seen.hold(Callee::SetReach);

    let answer = tokio::spawn({
        let host = host.clone();
        async move { call(&host, &address::Workspace, &set).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let held = answer.is_finished();
    seen.release(Callee::SetReach);

    assert!(!held, "a held call does not answer");
    assert_eq!(
        answer.await.unwrap().unwrap().version,
        ReachVersion(1)
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_read_opens_the_file_and_starts_the_actor_again() {
    let scratch = Scratch::new("realm-stand-ins");
    let seen = Arc::new(Seen::default());
    let host = open(&scratch, &seen, None).await;
    let workspace = address::Workspace.address().unwrap();

    let tables = stopped_open(
        &host,
        &scratch,
        &workspace,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
        |db| {
            db.query_row(
                "SELECT count(*) FROM sqlite_master",
                [],
                |row| row.get::<_, i64>(0),
            )
        },
    )
    .await
    .unwrap();

    assert!(tables >= 0);
    assert!(host.is_running(&workspace));
    host.shutdown().await.unwrap();
}
