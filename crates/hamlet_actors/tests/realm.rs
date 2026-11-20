//! The realm through a host, called as a client and as agents, with
//! stand-ins for the actors it calls.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use hamlet_actor::Action;
use hamlet_actor_core::ShutdownKind;
use hamlet_actor_error::ActorError;
use hamlet_actors::realm;
use hamlet_host::{Address, Host};
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::copies::Copies;
use hamlet_messages::agent::{AgentInput, Input, InputId, Submit};
use hamlet_messages::profile::{DreamId, EndDream};
use hamlet_messages::realm::budget::{
    BudgetLimit, Charge, MaySpend, SpendRefusal,
};
use hamlet_messages::realm::client::{Answer, Say};
use hamlet_messages::realm::profiles::{Dream, Dreamer};
use hamlet_messages::realm::world::{ReachSeen, Status};
use hamlet_messages::realm::{
    AgentName, Asked, Done, DoneTo, Found, FoundAnswer, HamletId,
    Hire, HireAnswer, HireRefusal, Named, NeedsChief, ProfileId,
    RoadDone, Sent, SetLimits, Tell, Tier, Tool,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::Need;
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId, ErrandNumber, ReachVersion,
    RealmTask,
};
use hamlet_protocol::client::{
    self as protocol, AgentId, Choice, RequestId,
};
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde::de::DeserializeOwned;
use stand_ins::{Forward, Seen, open};

#[path = "realm/allowance.rs"]
mod allowance;
#[path = "realm/asking.rs"]
mod asking;
#[path = "realm/budget.rs"]
mod budget;
#[path = "realm/callers.rs"]
mod callers;
#[path = "realm/chiefs.rs"]
mod chiefs;
#[path = "realm/choices.rs"]
mod choices;
#[path = "realm/copies.rs"]
mod copies;
#[path = "realm/dream_errands.rs"]
mod dream_errands;
#[path = "realm/dreams.rs"]
mod dreams;
#[path = "realm/drop_parts.rs"]
mod drop_parts;
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
#[path = "realm/librarian.rs"]
mod librarian;
#[path = "realm/living.rs"]
mod living;
#[path = "realm/names.rs"]
mod names;
#[path = "realm/oversight.rs"]
mod oversight;
#[path = "realm/parcels.rs"]
mod parcels;
#[path = "realm/places.rs"]
mod places;
#[path = "realm/profile_stand_in.rs"]
mod profile_stand_in;
#[path = "realm/profiles.rs"]
mod profiles;
#[path = "realm/prompt.rs"]
mod prompt;
#[path = "realm/record.rs"]
mod record;
#[path = "realm/refusals.rs"]
mod refusals;
#[path = "realm/repeats.rs"]
mod repeats;
#[path = "realm/reserved.rs"]
mod reserved;
#[path = "realm/reviews.rs"]
mod reviews;
#[path = "realm/revision.rs"]
mod revision;
#[path = "realm/say.rs"]
mod say;
#[path = "realm/secrets.rs"]
mod secrets;
#[path = "realm/settings.rs"]
mod settings;
#[path = "realm/society.rs"]
mod society;
#[path = "realm/stand_ins.rs"]
mod stand_ins;
#[path = "realm/steps.rs"]
mod steps;
#[path = "realm/tasks.rs"]
mod tasks;
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

    async fn stopped_rows<T>(
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
        version: None,
        purpose: Some("Check the links".to_owned()),
        name: None,
    }
}

fn hired<R>(agent: u32) -> HireAnswer<R> {
    HireAnswer::Hired {
        agent: shown(AgentNumber(agent)),
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

async fn made_until(realm: &Realm, agent: AgentNumber) -> AgentInput {
    until(&format!("agent {} is made", agent.0), || {
        made(realm, agent).is_some()
    })
    .await;
    made(realm, agent).unwrap()
}

fn standing(realm: &Realm, agent: AgentNumber) -> RealmTask {
    made(realm, agent).expect("the agent was made").task
}

fn errand(n: u32) -> AgentNumber {
    ErrandNumber::new(n).unwrap().agent()
}

/// Hire 3's name as the realm gives it.
const HIRE_3: &str = "hire#3 (Hire 1)";

fn of_hire(hire: AgentNumber) -> Dream {
    Dream {
        of: Dreamer::Hire { hire },
    }
}

/// The ends of dreams the profiles were asked for, in order.
fn dream_ends(realm: &Realm) -> Vec<(ProfileId, EndDream)> {
    realm.seen.dream_ends.lock().unwrap().clone()
}

/// The end of hire 3's dream `dream`.
fn hire_3_ends(dream: u32, done: bool, summary: &str) -> EndDream {
    EndDream {
        dream: DreamId(dream),
        done,
        summary: summary.to_owned(),
        hire_name: Some(HIRE_3.to_owned()),
    }
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

/// The words of a refusal, as the chief or the agent reads them.
fn words(answer: impl Serialize) -> String {
    let answer = serde_json::to_value(answer).unwrap();
    assert_eq!(answer["type"], "refused", "{answer}");
    answer["words"].as_str().unwrap().to_owned()
}

/// The answer of an agent's act on `agent`.
fn done_to<R>(agent: AgentNumber) -> DoneTo<R> {
    DoneTo::Done {
        agent: shown(agent),
    }
}

/// The answer of an agent's act on the road between `a` and `b`.
fn road_done<R>(a: AgentNumber, b: AgentNumber) -> RoadDone<R> {
    RoadDone::Done {
        a: shown(a),
        b: shown(b),
    }
}

/// The answer of a letter sent to `to`.
fn sent<R>(to: AgentNumber) -> Sent<R> {
    Sent::Sent { to: shown(to) }
}

/// `agent` as the realm names it to chiefs and agents, when the
/// mayor and each hire after it were made blank and unnamed in one
/// hamlet: the mayor as `Mayor`, the hires as `Hire 1` on.
fn shown(agent: AgentNumber) -> AgentName {
    let name = match agent.0 {
        1 => "hand".to_owned(),
        2 => "hire#2 (Mayor)".to_owned(),
        n => format!("hire#{n} (Hire {})", n - 2),
    };
    AgentName { agent, name }
}

/// The worker `worker`, which no one has nicknamed, as the realm
/// names it.
fn worker_shown(worker: AgentNumber) -> AgentName {
    AgentName {
        agent: worker,
        name: format!("worker#{}", worker.0),
    }
}

/// `agent` shown as `name`.
fn named_as(agent: u32, name: &str) -> AgentName {
    AgentName {
        agent: AgentNumber(agent),
        name: name.to_owned(),
    }
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

async fn told_of_failed(
    realm: &Realm,
    lead: AgentNumber,
    hire: AgentNumber,
) -> String {
    let id = InputId(format!("hire/{}", hire.0));
    let told = || {
        submits_to(realm, lead)
            .into_iter()
            .find(|seen| seen.id == id)
    };
    until(
        &format!("agent {} is told of hire {}", lead.0, hire.0),
        || told().is_some(),
    )
    .await;
    match told().map(|seen| seen.input) {
        Some(Input::Notice { text, .. }) => text,
        other => panic!("not a notice: {other:?}"),
    }
}

fn to(agent: AgentNumber) -> Named {
    Named::Agent { agent }
}

fn tell(to: Named, text: &str) -> Tell {
    Tell {
        to,
        text: text.to_owned(),
        paths: Vec::new(),
    }
}

fn needs(agent: AgentNumber, call: &str) -> NeedsChief {
    NeedsChief {
        agent,
        call: CallId(call.to_owned()),
        conversation: ConversationId("c".to_owned()),
        realm_task: None,
        needs: vec![Need::Command],
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
        .stopped_rows(|db| {
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
        task: None,
        conversation: None,
        act,
    }
}

/// `act` called in a turn on `task`.
fn paid<T>(call: &str, task: &RealmTask, act: T) -> Tool<T> {
    Tool {
        task: Some(task.clone()),
        ..tool(call, act)
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
