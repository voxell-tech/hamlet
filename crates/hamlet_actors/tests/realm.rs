//! The realm through a host, called as a client and as agents, with
//! stand-ins for the actors it calls.

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
use hamlet_messages::agent::copies::Copies;
use hamlet_messages::agent::{
    AgentInput, Fill, Place, PlaceVersion, Submit,
};
use hamlet_messages::profile::Branch;
use hamlet_messages::realm::budget::{
    BudgetLimit, Charge, MaySpend, SpendRefusal,
};
use hamlet_messages::realm::client::{Answer, Say};
use hamlet_messages::realm::world::{ReachSeen, Status};
use hamlet_messages::realm::{
    Asked, Done, Found, FoundAnswer, HamletId, Hire, HireAnswer,
    HireRefusal, Named, NeedsChief, ProfileId, SetLimits, Tell, Tier,
    Tool,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::Reach;
use hamlet_messages::workspace::{
    AgentNumber, AnswerTaken, CallId, ChiefAnswer, ConversationId,
    InForce, ReachVersion, RealmTask, SetReach,
};
use hamlet_protocol::client::{
    self as protocol, AgentId, Choice, RequestId,
};
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

#[path = "realm/allowance.rs"]
mod allowance;
#[path = "realm/asking.rs"]
mod asking;
#[path = "realm/budget.rs"]
mod budget;
#[path = "realm/chiefs.rs"]
mod chiefs;
#[path = "realm/choices.rs"]
mod choices;
#[path = "realm/errands.rs"]
mod errands;
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
#[path = "realm/oversight.rs"]
mod oversight;
#[path = "realm/parcels.rs"]
mod parcels;
#[path = "realm/places.rs"]
mod places;
#[path = "realm/record.rs"]
mod record;
#[path = "realm/reserved.rs"]
mod reserved;
#[path = "realm/reviews.rs"]
mod reviews;
#[path = "realm/say.rs"]
mod say;
#[path = "realm/secrets.rs"]
mod secrets;
#[path = "realm/settings.rs"]
mod settings;
#[path = "realm/society.rs"]
mod society;
#[path = "realm/steps.rs"]
mod steps;
#[path = "realm/waits.rs"]
mod waits;
#[path = "realm/workers.rs"]
mod workers;
#[path = "realm/world.rs"]
mod world;
#[path = "realm/writers.rs"]
mod writers;

const DEADLINE: Duration = Duration::from_secs(20);

const MAYOR: AgentNumber = AgentNumber(2);

const THREE: AgentNumber = AgentNumber(3);

const FOUR: AgentNumber = AgentNumber(4);

/// A call of the realm's that a test can hold or fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Callee {
    Branch,
    Create,
    Fill,
    Place,
    SetReach,
    Submit,
}

#[derive(Default)]
struct Seen {
    made: Mutex<Vec<(AgentNumber, AgentInput)>>,
    submits: Mutex<Vec<(AgentNumber, Submit)>>,
    places: Mutex<Vec<(AgentNumber, Place)>>,
    fills: Mutex<Vec<(AgentNumber, Fill)>>,
    copies: Mutex<Vec<(AgentNumber, Copies)>>,
    reaches: Mutex<Vec<SetReach>>,
    chief_answers: Mutex<Vec<ChiefAnswer>>,
    branches: Mutex<Vec<(ProfileId, Branch)>>,
    failing: Mutex<HashSet<(Callee, AgentNumber)>>,
    held: watch::Sender<HashSet<Callee>>,
    /// The calls that wait while the test holds their callee.
    waiting: Mutex<HashSet<(Callee, AgentNumber)>>,
}

impl Seen {
    /// Waits while the test holds `callee`, then fails it if the
    /// test asked it to fail once for `agent`.
    async fn called(
        &self,
        callee: Callee,
        agent: AgentNumber,
    ) -> anyhow::Result<()> {
        let mut held = self.held.subscribe();
        if held.borrow().contains(&callee) {
            self.waiting.lock().unwrap().insert((callee, agent));
        }
        let released =
            held.wait_for(|held| !held.contains(&callee)).await;
        self.waiting.lock().unwrap().remove(&(callee, agent));
        released?;
        if self.failing.lock().unwrap().remove(&(callee, agent)) {
            bail!(
                "the stand-in fails {callee:?} of {} once",
                agent.0
            );
        }
        Ok(())
    }

    fn fail_once(&self, callee: Callee, agent: AgentNumber) {
        self.failing.lock().unwrap().insert((callee, agent));
    }

    fn has_failed(&self, callee: Callee, agent: AgentNumber) -> bool {
        !self.failing.lock().unwrap().contains(&(callee, agent))
    }

    fn is_waiting(&self, callee: Callee, agent: AgentNumber) -> bool {
        self.waiting.lock().unwrap().contains(&(callee, agent))
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
    type Actions = (Submit, Place, Fill, Copies, Forward);
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
        self.seen.called(Callee::Create, self.agent).await?;
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
        self.seen.called(Callee::Submit, self.agent).await?;
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
        self.seen.called(Callee::Place, self.agent).await?;
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
        self.seen.called(Callee::Fill, self.agent).await?;
        self.seen.fills.lock().unwrap().push((self.agent, fill));
        Ok(())
    }
}

impl Handler<Copies> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        copies: Copies,
    ) -> anyhow::Result<()> {
        self.seen.copies.lock().unwrap().push((self.agent, copies));
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
        let agent = branch
            .name
            .strip_prefix("hires/")
            .context("a hire's branch is hires/<n>")?
            .parse()?;
        self.seen.called(Callee::Branch, AgentNumber(agent)).await?;
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
        self.seen.called(Callee::SetReach, set.agent).await?;
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

    async fn live(&self, agent: AgentNumber) {
        world::seen_until(
            self,
            &format!("agent {} is live", agent.0),
            |seen| {
                seen.agents.iter().any(|shown| {
                    shown.agent == agent
                        && shown.status == Status::Live
                        && matches!(
                            shown.reach,
                            ReachSeen::Applied { .. }
                        )
                })
            },
        )
        .await;
    }

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

async fn founded(realm: &Realm) -> HamletId {
    let answer = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: found("Mill"),
        })
        .await
        .unwrap();
    let FoundAnswer::Founded {
        hamlet,
        mayor: MAYOR,
    } = answer
    else {
        panic!("the chief founds a hamlet with mayor 2: {answer:?}");
    };
    hamlet
}

async fn hamlet_with(count: u32) -> (Realm, HamletId) {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let hamlet = founded(&realm).await;
    realm.live(MAYOR).await;
    for n in 0..count {
        hires(&realm, MAYOR, &format!("h{n}")).await;
        realm.live(AgentNumber(3 + n)).await;
    }
    (realm, hamlet)
}

fn hire(under: Option<Named>) -> Hire {
    Hire {
        hamlet: None,
        under,
        profile: None,
        purpose: Some("Check the links".to_owned()),
        name: None,
    }
}

fn hired<R>(agent: u32) -> HireAnswer<R> {
    HireAnswer::Hired {
        agent: AgentNumber(agent),
    }
}

async fn hires(realm: &Realm, lead: AgentNumber, call: &str) {
    let hired = realm.tool(lead, &tool(call, hire(None))).await;
    assert!(
        matches!(hired, Ok(HireAnswer::Hired { .. })),
        "{hired:?}"
    );
}

async fn chief_hires(
    realm: &Realm,
    hamlet: &HamletId,
    act: Hire,
) -> HireAnswer<HireRefusal> {
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act: Hire {
                hamlet: Some(hamlet.clone()),
                ..act
            },
        })
        .await
        .unwrap()
}

fn made(realm: &Realm, agent: AgentNumber) -> Option<AgentInput> {
    realm
        .seen
        .made
        .lock()
        .unwrap()
        .iter()
        .find(|(made, _)| *made == agent)
        .map(|(_, input)| input.clone())
}

fn standing(realm: &Realm, agent: AgentNumber) -> RealmTask {
    made(realm, agent).expect("the agent was made").task
}

fn charge(
    call: &str,
    task: &RealmTask,
    micros: u32,
    ms: u32,
) -> Charge {
    Charge {
        call: CallId(call.to_owned()),
        task: task.clone(),
        micros,
        took_ms: ms,
        tokens: None,
    }
}

async fn may_spend(
    realm: &Realm,
    agent: AgentNumber,
    task: &RealmTask,
) -> Done<SpendRefusal> {
    let may = MaySpend { task: task.clone() };
    realm.tool(agent, &may).await.unwrap()
}

async fn limit(
    realm: &Realm,
    hamlet: Option<&HamletId>,
    budget: Vec<BudgetLimit>,
) {
    let set = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: SetLimits {
                hamlet: hamlet.cloned(),
                budget,
                limits: Vec::new(),
            },
        })
        .await
        .unwrap();
    assert_eq!(set, Done::Done);
}

/// The copies `agent` was pushed, oldest first.
fn copies_to(realm: &Realm, agent: AgentNumber) -> Vec<Copies> {
    realm
        .seen
        .copies
        .lock()
        .unwrap()
        .iter()
        .filter(|(to, _)| *to == agent)
        .map(|(_, copies)| copies.clone())
        .collect()
}

fn submits_to(realm: &Realm, agent: AgentNumber) -> Vec<Submit> {
    realm
        .seen
        .submits
        .lock()
        .unwrap()
        .iter()
        .filter(|(to, _)| *to == agent)
        .map(|(_, submit)| submit.clone())
        .collect()
}

async fn seen_submits(
    realm: &Realm,
    agent: AgentNumber,
    count: usize,
) -> Vec<Submit> {
    until(&format!("agent {} saw {count} inputs", agent.0), || {
        submits_to(realm, agent).len() >= count
    })
    .await;
    submits_to(realm, agent)
}

fn to(agent: AgentNumber) -> Named {
    Named::Agent { agent }
}

fn tell(to: Named, text: &str) -> Tell {
    Tell {
        to,
        text: text.to_owned(),
        task: None,
        paths: Vec::new(),
    }
}

fn needs(agent: AgentNumber, call: &str) -> NeedsChief {
    NeedsChief {
        agent,
        call: CallId(call.to_owned()),
        conversation: ConversationId("c".to_owned()),
        realm_task: None,
        needs: Vec::new(),
        subject: "rm -rf build".to_owned(),
        change: None,
        reach_version: ReachVersion(1),
    }
}

fn answer(wait: &str, choice: Choice, text: Option<&str>) -> Answer {
    Answer(protocol::Answer {
        choice,
        id: RequestId::new(),
        picks: None,
        text: text.map(str::to_owned),
        wait: wait.to_owned(),
    })
}

fn say(id: &RequestId, to: AgentNumber, text: &str) -> Say {
    Say(protocol::Say {
        files: Vec::new(),
        id: id.clone(),
        images: Vec::new(),
        text: text.to_owned(),
        to: AgentId(u64::from(to.0)),
    })
}

async fn purpose_of(
    realm: &Realm,
    agent: AgentNumber,
) -> Option<String> {
    realm
        .rows(|db| {
            db.query_row(
                "SELECT purpose FROM hire WHERE agent = ?1",
                [agent.0],
                |row| row.get(0),
            )
        })
        .await
}

fn tool<T>(call: &str, act: T) -> Tool<T> {
    Tool {
        call: CallId(call.to_owned()),
        act,
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
    seen.fail_once(Callee::SetReach, AgentNumber(2));
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
        folder: None,
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
