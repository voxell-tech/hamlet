//! A stand-in for the realm: the actor `realm/local`, which sets
//! agents' reach on the workspace by calling it as itself, answers
//! the workspace's call for its secrets with one made-up secret,
//! `demo-secret-value`, of version 1, and keeps each call the
//! workspace says waits for a chief in its `waits` table.
//!
//! ```text
//! demo realm reach <n> <version> [project=<path>] [mode=manual|auto]
//!     [net] [worker | worker=<starter>]
//!     [trust=<conversation>:edits,commands] [secrets=<v>] [ended]
//!                    set agent <n>'s reach; manual mode by default
//! demo realm answer <n> <call> allow|refuse
//!     |refuse=<reason>|stopped|rewound|ended
//!                    answer agent <n>'s call <call> as a chief, or
//!                    end its wait
//! ```
//!
//! `reach` prints `reach/<n> <v>`, the version in force the workspace
//! answered with. A wait the workspace tells of prints
//! `waits/<n>/<call> <needs>`, how many things it needs, and logs its
//! subject. `answer` prints `answered/<call> 1` when the call runs,
//! and else `answered/<call>/not_run 0`, logging why, or
//! `answered/<call>/no_wait 0`.
//!
//! With `--realm-actor`, the demo runs the realm actor in place of
//! this stand-in and of the stub agent, with the real workspace, a
//! stand-in agent `agent/<n>` that the realm makes, and a stand-in
//! profile `profile/<n>` that answers each branch done. These are its
//! commands:
//!
//! ```text
//! demo realm found <name>
//!                    found the hamlet <name> as the chief
//! demo realm hire <lead> <call> <purpose ...>
//!                    have agent <lead> hire, as its call <call>
//! demo realm say <n> <id> <text ...>
//!                    say a line to agent <n> as the chief, under the
//!                    request id the word <id> stands for
//! demo realm answer <wait> allow|refuse
//!                    answer the wait <wait> as the chief
//! demo realm end <n>
//!                    end agent <n> as the chief
//! demo realm budget <hamlet> <micros>
//!                    bound the hamlet <hamlet>'s spending
//! demo realm world
//!                    read the world a client shows
//! demo realm reads <count> <ms> [<n>]
//!                    read the world <count> times, <ms> apart
//! demo agent <n> tell <call> <to> <text ...>
//! demo agent <n> ask <call> <to> <text ...>
//!                    send a letter or a question as agent <n>'s call
//!                    <call>, to `hand`, `mayor` or `hire#<k>`
//! demo agent <n> reply <question> <text ...>
//!                    answer the question <question>
//! demo agent <n> may-spend [<task>]
//! demo agent <n> charge <call> <micros> [<task>]
//!                    ask whether agent <n> may spend, or charge its
//!                    model call <call>, for <task> or its standing task
//! demo agent <n> spend <micros> <times>
//!                    up to <times> times, ask and charge <micros>
//! demo agent <n> letters <count> <to>
//!                    tell <to> <count> letters, one after another
//! demo agent <n> run <call> [net] <script ...>
//!                    run a script as agent <n>'s call <call>, with
//!                    the network when `net` is given
//! demo agent <n> inputs
//!                    list the inputs agent <n> took
//! demo agent <first>..<last> ...
//!                    run the command as each agent at once
//! demo ... &         run the command in the background
//! ```
//!
//! Each prints `<name> <i64>` lines: `founded/<name> <mayor>`,
//! `hired/<call> <agent>`, `taken/<id> <task>` or `not_taken/<id>
//! <task>`, `answered/<wait> 1`, `ended/<n> 1`, `budget/<hamlet>
//! <micros>`, `sent/<call> 1`, `replied/<question> 1`, `may/<n> 1`
//! or `0`, `charged/<call> <micros>`, `spent/<n> <micros>`,
//! `told/<n> <count>`, and for `run`, `done/<call>/<exit> <code>`,
//! `running/<call> <command>` or `waits/<call> <needs>`. A refusal
//! prints `refused/<name> 1` and logs why. `world` prints `world
//! <revision>`, then `hamlet/<name> <mayor>`, `agent/<n>/<status>
//! <lead>`, `reach/<n>/<none|applied|pending> <version>`,
//! `wait/<n>/<call> <wait>`, and `spend/<payer> <micros>` with
//! `budget/<payer> <micros>` where a budget bounds it.
//! `reads` prints `read/<i> <us>`, the read's time, with `<n>`'s
//! reach whenever it changes, and then `reads/longest <us>`.
//! `inputs` prints `standing/<n> <task>`, then `input/<n>/<taken>/
//! <kind> <task>` in the order taken, and `question/<n>/<taken>
//! <letter>` for a question. The stand-in agent keeps its inputs, its
//! newest place and the ends the workspace tells it of in its file,
//! and prints `finished/<call>/<exit> <code>` for each end.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{
    ActorConfig, ActorFactory, ActorKeySegment, BindParam,
    ColumnValue,
};
use hamlet_actors::realm;
use hamlet_host::{Caller, Host, HostRef};
use hamlet_messages::Message;
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::{
    AgentInput, AgentKind, Fill, Finished, Input, Outcome, Place,
    Submit,
};
use hamlet_messages::profile::Branch;
use hamlet_messages::realm::budget::{
    BudgetLimit, Charge, MaySpend, Payer,
};
use hamlet_messages::realm::client::{
    self, AnswerRefusal, ReadWorld, Say, SayAnswer,
};
use hamlet_messages::realm::world::{ReachSeen, Seen, Status};
use hamlet_messages::realm::{
    AgentRefusal, Ask, ChiefRequest, Done, End, Found, FoundAnswer,
    Hire, HireAnswer, LetterId, LetterRefusal, Named, NeedsChief,
    Reply, SecretList, Secrets, SetLimits, Tell, Tier, Tool,
};
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::reach::{
    Footing, Mode, Project, Reach, SecretsVersion, Trust,
};
use hamlet_messages::workspace::{
    AgentNumber, Answer, AnswerTaken, CallId, ChiefAnswer,
    ConversationId, Exit, InForce, Lapse, Program, ReachVersion,
    RealmTask, Run, RunAnswer, RunEnd, SetReach,
};

use crate::command::{number, shown};

/// The kind `realm`.
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

struct Realm;

/// Asks the realm to set an agent's reach on the workspace.
/// The program's own action, so it checks no sender.
#[derive(serde::Serialize, serde::Deserialize)]
struct Grant(SetReach);

impl Action for Grant {
    type Output = InForce;
    const NAME: &'static str = "grant";
}

/// Asks the realm to answer a call that waits for a chief.
/// The program's own action, so it checks no sender.
#[derive(serde::Serialize, serde::Deserialize)]
struct Decide(ChiefAnswer);

impl Action for Decide {
    type Output = AnswerTaken;
    const NAME: &'static str = "decide";
}

#[async_trait::async_trait]
impl Actor for Realm {
    type Input = ();
    type Actions = (Grant, Decide, Secrets, NeedsChief);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        ctx.sql()
            .exec(
                "CREATE TABLE IF NOT EXISTS waits (agent INTEGER \
                 NOT NULL, call TEXT NOT NULL, needs INTEGER NOT \
                 NULL, subject TEXT NOT NULL) STRICT",
            )
            .await?;
        Ok(Self)
    }
}

impl Handler<Grant> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Grant(set): Grant,
    ) -> Result<InForce> {
        call(
            &HostRef::of(ctx.inner())?,
            &address::Workspace,
            &set,
            CALL_WITHIN,
        )
        .await
    }
}

impl Handler<Decide> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Decide(answer): Decide,
    ) -> Result<AnswerTaken> {
        call(
            &HostRef::of(ctx.inner())?,
            &address::Workspace,
            &answer,
            CALL_WITHIN,
        )
        .await
    }
}

const CALL_WITHIN: Duration = Duration::from_secs(10);

async fn call<M: Message>(
    host: &HostRef,
    to: &M::To,
    message: &M,
    deadline: Duration,
) -> Result<M::Output> {
    let args = hamlet_actor::action::encode_positional(message)?;
    let answer =
        host.call(&to.address()?, M::NAME, args, deadline).await?;
    Ok(ciborium::from_reader(answer.as_slice())?)
}

impl Handler<NeedsChief> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: NeedsChief,
    ) -> Result<()> {
        sender(&ctx, &asked)?;
        let needs = i64::try_from(asked.needs.len())?;
        ctx.sql()
            .execute(
                "INSERT INTO waits (agent, call, needs, subject) \
                 VALUES (?1, ?2, ?3, ?4)",
                Some(vec![
                    BindParam::Integer(i64::from(asked.agent.0)),
                    BindParam::Text(asked.call.0.clone()),
                    BindParam::Integer(needs),
                    BindParam::Text(asked.subject.clone()),
                ]),
            )
            .await?;
        tracing::info!(
            subject = asked.subject,
            needs = ?asked.needs,
            "agent {}'s call {} waits for a chief",
            asked.agent.0,
            asked.call.0
        );
        writeln!(
            std::io::stdout(),
            "waits/{}/{} {needs}",
            asked.agent.0,
            asked.call.0
        )?;
        Ok(())
    }
}

impl Handler<Secrets> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Secrets,
    ) -> Result<SecretList> {
        sender(&ctx, &asked)?;
        Ok(SecretList {
            version: SecretsVersion(1),
            values: vec!["demo-secret-value".to_owned()],
        })
    }
}

/// Runs `demo realm <words>`.
pub async fn run(host: &Host, words: &[String]) -> Result<()> {
    if let [verb, agent, call, answer] = words
        && verb == "answer"
    {
        return answer_call(host, agent, call, answer).await;
    }
    let [verb, agent, version, rest @ ..] = words else {
        bail!(
            "usage: demo realm reach <n> <version> ... | answer <n> \
             <call> <answer>"
        );
    };
    if verb != "reach" {
        bail!("the realm takes no command `{verb}`");
    }
    let set = SetReach {
        agent: AgentNumber(agent.parse()?),
        version: ReachVersion(version.parse()?),
        reach: parse_reach(rest)?,
    };
    host.get_or_start(&address::Workspace.address()?).await?;
    host.get_or_start(&address::Realm.address()?).await?;
    let reply = host
        .call(
            &address::Realm.address()?,
            Grant::NAME,
            hamlet_actor::action::encode_positional(&Grant(set))?,
            Duration::from_secs(20),
        )
        .await?;
    let in_force =
        ciborium::from_reader::<InForce, _>(reply.as_slice())?;
    writeln!(
        std::io::stdout(),
        "reach/{agent} {}",
        in_force.version.0
    )?;
    Ok(())
}

fn parse_reach(words: &[String]) -> Result<Reach> {
    let mut footing = Footing {
        project: None,
        network: false,
        trust: Vec::new(),
        secrets: SecretsVersion(1),
    };
    let mut mode = Mode::Manual;
    let mut worker = None;
    for word in words {
        match word.split_once('=') {
            None if word == "ended" => return Ok(Reach::Ended),
            None if word == "net" => footing.network = true,
            None if word == "worker" => worker = Some(None),
            Some(("project", folder)) => {
                footing.project = Some(Project {
                    folder: folder.into(),
                    mode,
                });
            }
            Some(("mode", "manual")) => mode = Mode::Manual,
            Some(("mode", "auto")) => mode = Mode::Auto,
            Some(("worker", starter)) => {
                worker = Some(Some(AgentNumber(starter.parse()?)));
            }
            Some(("secrets", version)) => {
                footing.secrets = SecretsVersion(version.parse()?);
            }
            Some(("trust", trust)) => {
                let (conversation, allows) =
                    trust.split_once(':').context(
                        "trust is <conversation>:edits,commands",
                    )?;
                footing.trust.push(Trust {
                    conversation: ConversationId(conversation.into()),
                    edits: allows.split(',').any(|it| it == "edits"),
                    commands: allows
                        .split(',')
                        .any(|it| it == "commands"),
                });
            }
            _ => bail!("`{word}` is not a part of a reach"),
        }
    }
    if let Some(project) = &mut footing.project {
        project.mode = mode;
    }
    Ok(match worker {
        Some(starter) => Reach::Worker { starter, footing },
        None => Reach::Agent(footing),
    })
}

/// Runs `demo realm answer <agent> <call> <answer>`.
async fn answer_call(
    host: &Host,
    agent: &str,
    call: &str,
    answer: &str,
) -> Result<()> {
    let answer = match answer.split_once('=') {
        Some(("refuse", reason)) => Answer::Refuse {
            reason: Some(reason.to_owned()),
        },
        _ => match answer {
            "allow" => Answer::Allow,
            "refuse" => Answer::Refuse { reason: None },
            "stopped" => Answer::Lapsed {
                lapse: Lapse::Stopped,
            },
            "rewound" => Answer::Lapsed {
                lapse: Lapse::Rewound,
            },
            "ended" => Answer::Lapsed {
                lapse: Lapse::Ended,
            },
            _ => bail!("`{answer}` is not an answer"),
        },
    };
    let decide = Decide(ChiefAnswer {
        agent: AgentNumber(agent.parse()?),
        call: CallId(call.to_owned()),
        answer,
    });
    host.get_or_start(&address::Workspace.address()?).await?;
    host.get_or_start(&address::Realm.address()?).await?;
    let reply = host
        .call(
            &address::Realm.address()?,
            Decide::NAME,
            hamlet_actor::action::encode_positional(&decide)?,
            Duration::from_secs(20),
        )
        .await?;
    let mut out = std::io::stdout();
    match ciborium::from_reader(reply.as_slice())? {
        AnswerTaken::Taken => writeln!(out, "answered/{call} 1")?,
        AnswerTaken::RunNotRun { why } => {
            tracing::warn!(%why, "agent {agent}'s call {call} did not run");
            writeln!(out, "answered/{call}/not_run 0")?;
        }
        AnswerTaken::ChangeNotRun { why } => {
            tracing::warn!(%why, "agent {agent}'s call {call} did not run");
            writeln!(out, "answered/{call}/not_run 0")?;
        }
        AnswerTaken::NoWait => {
            writeln!(out, "answered/{call}/no_wait 0")?;
        }
    }
    Ok(())
}

pub fn actor_kinds() -> [(String, ActorFactory); 3] {
    [
        (address::Realm::KIND.to_owned(), realm::factory()),
        (
            address::Agent::KIND.to_owned(),
            kind::<StandInAgent>(address::Agent::KIND, true),
        ),
        (
            address::Profile::KIND.to_owned(),
            kind::<StandInProfile>(address::Profile::KIND, false),
        ),
    ]
}

fn kind<A: Actor>(name: &str, has_database: bool) -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<A>(
        name,
        ActorConfig {
            has_database,
            ..ActorConfig::default()
        },
    );
    registry
        .into_factories()
        .remove(name)
        .expect("the registry holds the kind it registered")
}

struct StandInAgent;

const AGENT_TABLES: &str =
    "CREATE TABLE IF NOT EXISTS standing (id INTEGER PRIMARY KEY \
     CHECK (id = 1), kind TEXT NOT NULL, task TEXT NOT NULL) \
     STRICT; CREATE TABLE IF NOT EXISTS place (id INTEGER PRIMARY \
     KEY CHECK (id = 1), version INTEGER NOT NULL, lead INTEGER, \
     hires TEXT NOT NULL, roads TEXT NOT NULL) STRICT; CREATE TABLE \
     IF NOT EXISTS input (taken INTEGER PRIMARY KEY, id TEXT NOT \
     NULL UNIQUE, kind TEXT NOT NULL, text TEXT NOT NULL, task \
     TEXT, sender INTEGER, letter INTEGER, at INTEGER NOT NULL) \
     STRICT; CREATE TABLE IF NOT EXISTS finished (call TEXT NOT \
     NULL, exit TEXT NOT NULL, code INTEGER NOT NULL, at INTEGER \
     NOT NULL) STRICT;";

#[async_trait::async_trait]
impl Actor for StandInAgent {
    type Input = Option<AgentInput>;
    type Actions = (Submit, Place, Fill, Finished, Forward, Kept);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        ctx.sql().exec(AGENT_TABLES).await?;
        Ok(Self)
    }

    async fn on_create(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> Result<()> {
        let input = ctx.input()?.context(
            "only the realm makes an agent, with its input",
        )?;
        let kind = match input.kind {
            AgentKind::Hand => "hand",
            AgentKind::Hire => "hire",
            AgentKind::Worker => "worker",
            AgentKind::Errand => "errand",
        };
        ctx.sql()
            .execute(
                "INSERT INTO standing (id, kind, task) VALUES (1, \
                 ?1, ?2)",
                Some(vec![
                    BindParam::Text(kind.to_owned()),
                    BindParam::Text(input.task.0),
                ]),
            )
            .await?;
        keep_place(&ctx, &input.place).await
    }
}

impl Handler<Submit> for StandInAgent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        submit: Submit,
    ) -> Result<()> {
        sender(&ctx, &submit)?;
        let (kind, text, from, letter) = match &submit.input {
            Input::Line { text, .. } => ("line", text, None, None),
            Input::Notice { text, .. } => {
                ("notice", text, None, None)
            }
            Input::WorkerAnswer { text, .. } => {
                ("worker_answer", text, None, None)
            }
            Input::Letter { from, text } => {
                ("letter", text, Some(from), None)
            }
            Input::Question { letter, from, text } => {
                ("question", text, Some(from), Some(letter))
            }
            Input::Answer {
                question,
                from,
                text,
                ..
            } => ("answer", text, Some(from), Some(question)),
        };
        let kept = ctx
            .sql()
            .execute(
                "INSERT INTO input (id, kind, text, task, sender, \
                 letter, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON \
                 CONFLICT (id) DO NOTHING",
                Some(vec![
                    BindParam::Text(submit.id.0.clone()),
                    BindParam::Text(kind.to_owned()),
                    BindParam::Text(text.clone()),
                    submit
                        .task
                        .as_ref()
                        .map_or(BindParam::Null, |task| {
                            BindParam::Text(task.0.clone())
                        }),
                    from.map_or(BindParam::Null, |from| {
                        BindParam::Integer(i64::from(from.agent.0))
                    }),
                    letter.map_or(BindParam::Null, |letter| {
                        BindParam::Integer(i64::from(letter.0))
                    }),
                    BindParam::Integer(now_ms()?),
                ]),
            )
            .await?;
        tracing::info!(
            id = submit.id.0,
            kind,
            text,
            repeat = kept.changes == 0,
            "{} took an input",
            ctx.actor_id()
        );
        Ok(())
    }
}

impl Handler<Place> for StandInAgent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        place: Place,
    ) -> Result<()> {
        sender(&ctx, &place)?;
        keep_place(&ctx, &place).await
    }
}

async fn keep_place(
    ctx: &Ctx<StandInAgent>,
    place: &Place,
) -> Result<()> {
    let listed = |agents: &[AgentNumber]| {
        agents
            .iter()
            .map(|agent| agent.0.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    ctx.sql()
        .execute(
            "INSERT INTO place (id, version, lead, hires, roads) \
             VALUES (1, ?1, ?2, ?3, ?4) ON CONFLICT (id) DO UPDATE \
             SET version = excluded.version, lead = excluded.lead, \
             hires = excluded.hires, roads = excluded.roads WHERE \
             excluded.version > place.version",
            Some(vec![
                BindParam::Integer(i64::try_from(place.version.0)?),
                place.lead.map_or(BindParam::Null, |lead| {
                    BindParam::Integer(i64::from(lead.0))
                }),
                BindParam::Text(listed(&place.hires)),
                BindParam::Text(listed(&place.roads)),
            ]),
        )
        .await?;
    Ok(())
}

impl Handler<Fill> for StandInAgent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        fill: Fill,
    ) -> Result<()> {
        sender(&ctx, &fill)?;
        let Some(ActorKeySegment::String(key)) = ctx.key().first()
        else {
            bail!("an agent's key is its number");
        };
        let folder = HostRef::of(ctx.inner())?
            .folder()
            .join(format!("actors/agent/{key}.fs"));
        std::fs::create_dir_all(folder)?;
        Ok(())
    }
}

impl Handler<Finished> for StandInAgent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        finished: Finished,
    ) -> Result<()> {
        sender(&ctx, &finished)?;
        let call = &finished.call.0;
        let (exit, code) = match &finished.outcome {
            Outcome::Run {
                end: RunEnd::Ran(end),
            } => exit_word(&end.exit),
            Outcome::Run {
                end: RunEnd::NotRun { why },
            } => {
                tracing::warn!(%why, "{}'s call {call} did not run", ctx.actor_id());
                ("not_run", 1)
            }
            Outcome::Change { end } => {
                tracing::info!(
                    ?end,
                    "{}'s change {call} ended",
                    ctx.actor_id()
                );
                ("change", 1)
            }
        };
        ctx.sql()
            .execute(
                "INSERT INTO finished (call, exit, code, at) VALUES \
                 (?1, ?2, ?3, ?4)",
                Some(vec![
                    BindParam::Text(call.clone()),
                    BindParam::Text(exit.to_owned()),
                    BindParam::Integer(code),
                    BindParam::Integer(now_ms()?),
                ]),
            )
            .await?;
        writeln!(std::io::stdout(), "finished/{call}/{exit} {code}")?;
        Ok(())
    }
}

fn exit_word(exit: &Exit) -> (&'static str, i64) {
    match exit {
        Exit::Code { code } => ("exit", i64::from(*code)),
        Exit::TimedOut => ("timed_out", 0),
        Exit::Failed { .. } => ("failed", 0),
        Exit::CutOff => ("cut_off", 0),
        Exit::Cancelled => ("cancelled", 0),
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Forward {
    to: Peer,
    name: String,
    args: Vec<u8>,
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
enum Peer {
    Realm,
    Workspace,
}

impl Action for Forward {
    type Output = Vec<u8>;
    const NAME: &'static str = "forward";
}

impl Handler<Forward> for StandInAgent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        forward: Forward,
    ) -> Result<Vec<u8>> {
        let to = match forward.to {
            Peer::Realm => address::Realm.address()?,
            Peer::Workspace => address::Workspace.address()?,
        };
        HostRef::of(ctx.inner())?
            .call(&to, &forward.name, forward.args, DRIVE_WITHIN)
            .await
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Kept;

#[derive(serde::Serialize, serde::Deserialize)]
struct Holding {
    standing: RealmTask,
    inputs: Vec<Taken>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Taken {
    taken: i64,
    kind: String,
    task: Option<RealmTask>,
    letter: Option<i64>,
}

impl Action for Kept {
    type Output = Holding;
    const NAME: &'static str = "kept";
}

impl Handler<Kept> for StandInAgent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Kept,
    ) -> Result<Holding> {
        let standing = ctx
            .sql()
            .query("SELECT task FROM standing", None)
            .await?
            .rows
            .first()
            .and_then(|row| text(row.first()?))
            .context("the agent keeps its standing task")?;
        let inputs = ctx
            .sql()
            .query(
                "SELECT taken, kind, task, letter FROM input ORDER \
                 BY taken",
                None,
            )
            .await?
            .rows
            .iter()
            .map(|row| match row.as_slice() {
                [ColumnValue::Integer(taken), kind, task, letter] => {
                    Ok(Taken {
                        taken: *taken,
                        kind: text(kind)
                            .context("an input has a kind")?,
                        task: text(task).map(RealmTask),
                        letter: match letter {
                            ColumnValue::Integer(letter) => {
                                Some(*letter)
                            }
                            _ => None,
                        },
                    })
                }
                _ => bail!("an input row reads as it was written"),
            })
            .collect::<Result<_>>()?;
        Ok(Holding {
            standing: RealmTask(standing),
            inputs,
        })
    }
}

fn text(value: &ColumnValue) -> Option<String> {
    match value {
        ColumnValue::Text(text) => Some(text.clone()),
        _ => None,
    }
}

struct StandInProfile;

#[async_trait::async_trait]
impl Actor for StandInProfile {
    type Input = ();
    type Actions = (Branch,);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(_: &Ctx<Self>) -> Result<Self> {
        Ok(Self)
    }
}

impl Handler<Branch> for StandInProfile {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        branch: Branch,
    ) -> Result<()> {
        sender(&ctx, &branch)?;
        Ok(())
    }
}

const DRIVE_WITHIN: Duration = Duration::from_secs(20);

const CONVERSATION: &str = "demo";

pub async fn drive(host: &Host, words: &[String]) -> Result<()> {
    let [line @ .., last] = words else {
        bail!("a command takes words");
    };
    if last != "&" {
        return drive_now(host, words).await;
    }
    let host = host.clone();
    let line = line.to_vec();
    tokio::spawn(async move {
        if let Err(error) = drive_now(&host, &line).await {
            let _ = writeln!(
                std::io::stdout(),
                "error: {}",
                shown(&error)
            );
        }
    });
    Ok(())
}

async fn drive_now(host: &Host, words: &[String]) -> Result<()> {
    host.get_or_start(&address::Workspace.address()?).await?;
    host.get_or_start(&address::Realm.address()?).await?;
    match words {
        [first, rest @ ..] if first == "realm" => {
            as_chief(host, rest).await
        }
        [first, agents, rest @ ..] if first == "agent" => {
            let Some((first, last)) = agents.split_once("..") else {
                return as_agent(host, agent_number(agents)?, rest)
                    .await;
            };
            let mut runs = tokio::task::JoinSet::new();
            for me in agent_number(first)?..=agent_number(last)? {
                let host = host.clone();
                let rest = rest.to_vec();
                runs.spawn(async move {
                    as_agent(&host, me, &rest).await
                });
            }
            while let Some(ran) = runs.join_next().await {
                ran??;
            }
            Ok(())
        }
        _ => bail!("usage: demo realm ... | agent <n> ..."),
    }
}

async fn as_chief(host: &Host, words: &[String]) -> Result<()> {
    let mut out = std::io::stdout();
    match words {
        [verb, name] if verb == "found" => {
            let found = Found {
                name: name.clone(),
                mayor: None,
                tier: Tier::Fifty,
                purpose: None,
            };
            match request(host, found).await? {
                FoundAnswer::Founded { mayor, .. } => {
                    writeln!(out, "founded/{name} {}", mayor.0)?;
                }
                FoundAnswer::Refused { why } => refused(name, &why)?,
            }
        }
        [verb, lead, call, purpose @ ..]
            if verb == "hire" && !purpose.is_empty() =>
        {
            let hire = Hire {
                hamlet: None,
                under: None,
                profile: None,
                purpose: Some(purpose.join(" ")),
                name: None,
            };
            let tool = tool(call, hire);
            match forward(
                host,
                agent_number(lead)?,
                Peer::Realm,
                &tool,
            )
            .await?
            {
                HireAnswer::Hired { agent } => {
                    writeln!(out, "hired/{call} {}", agent.0)?;
                }
                HireAnswer::Refused { why } => refused(call, &why)?,
            }
        }
        [verb, agent, id, text @ ..] if verb == "say" => {
            let line = Line {
                id: request_id(id),
                text: text.join(" "),
                to: number(Some(agent))?,
            };
            tracing::info!(id = line.id, "the request id of `{id}`");
            match as_client(host, Say::NAME, &line).await? {
                SayAnswer::Taken { task } => {
                    writeln!(
                        out,
                        "taken/{id} {}",
                        task_number(&task)
                    )?;
                }
                SayAnswer::NotTaken { task } => writeln!(
                    out,
                    "not_taken/{id} {}",
                    task_number(&task)
                )?,
                SayAnswer::Refused { why } => refused(id, &why)?,
            }
        }
        [verb, wait, choice] if verb == "answer" => {
            let choice = Choice {
                choice: match choice.as_str() {
                    "allow" => "allow",
                    "refuse" => "deny",
                    _ => bail!("an answer is allow or refuse"),
                },
                id: fresh_id()?,
                wait: wait.clone(),
            };
            match as_client(host, client::Answer::NAME, &choice)
                .await?
            {
                Done::<AnswerRefusal>::Done => {
                    writeln!(out, "answered/{wait} 1")?;
                }
                Done::Refused { why } => refused(wait, &why)?,
            }
        }
        [verb, agent] if verb == "end" => {
            let end = End {
                hire: Named::Agent {
                    agent: AgentNumber(agent_number(agent)?),
                },
                why: None,
            };
            match request(host, end).await? {
                Done::Done => writeln!(out, "ended/{agent} 1")?,
                Done::Refused { why } => refused(agent, &why)?,
            }
        }
        [verb, name, micros] if verb == "budget" => {
            let seen = read_world(host).await?;
            let hamlet = seen
                .hamlets
                .iter()
                .find(|hamlet| hamlet.name == *name)
                .with_context(|| {
                    format!("no hamlet is named `{name}`")
                })?;
            let limits = SetLimits {
                hamlet: Some(hamlet.hamlet.clone()),
                budget: vec![BudgetLimit::Cost {
                    micros: number(Some(micros))?,
                }],
            };
            match request(host, limits).await? {
                Done::Done => {
                    writeln!(out, "budget/{name} {micros}")?;
                }
                Done::Refused { why } => refused(name, &why)?,
            }
        }
        [verb] if verb == "world" => {
            print_world(&read_world(host).await?)?;
        }
        [verb, count, every, watched @ ..] if verb == "reads" => {
            let every = Duration::from_millis(number(Some(every))?);
            let watched = watched
                .first()
                .map(|agent| agent_number(agent))
                .transpose()?;
            let mut longest = Duration::ZERO;
            let mut shown = None;
            for read in 1..=number(Some(count))? {
                let begun = Instant::now();
                let seen = read_world(host).await?;
                let took = begun.elapsed();
                longest = longest.max(took);
                writeln!(out, "read/{read} {}", took.as_micros())?;
                let reach = watched.and_then(|agent| {
                    seen.agents
                        .iter()
                        .find(|seen| seen.agent.0 == agent)
                        .map(|seen| (agent, reach_word(&seen.reach)))
                });
                if let Some((agent, (word, version))) = reach
                    && shown != Some((word, version))
                {
                    writeln!(out, "reach/{agent}/{word} {version}")?;
                    shown = Some((word, version));
                }
                tokio::time::sleep(every).await;
            }
            writeln!(out, "reads/longest {}", longest.as_micros())?;
        }
        _ => bail!(
            "usage: demo realm \
             found|hire|say|answer|end|budget|world|reads ..."
        ),
    }
    Ok(())
}

async fn as_agent(
    host: &Host,
    me: u32,
    words: &[String],
) -> Result<()> {
    let mut out = std::io::stdout();
    match words {
        [verb, call, to, text @ ..] if verb == "tell" => {
            match tell(host, me, call, named(to)?, text.join(" "))
                .await?
            {
                Done::Done => writeln!(out, "sent/{call} 1")?,
                Done::Refused { why } => refused(call, &why)?,
            }
        }
        [verb, call, to, text @ ..] if verb == "ask" => {
            let ask = Ask {
                to: named(to)?,
                text: text.join(" "),
                conversation: ConversationId(CONVERSATION.to_owned()),
                task: None,
                paths: Vec::new(),
            };
            match forward(host, me, Peer::Realm, &tool(call, ask))
                .await?
            {
                Done::Done => writeln!(out, "sent/{call} 1")?,
                Done::Refused { why } => refused(call, &why)?,
            }
        }
        [verb, question, text @ ..] if verb == "reply" => {
            let reply = Reply {
                question: LetterId(u32::try_from(number(Some(
                    question,
                ))?)?),
                text: text.join(" "),
            };
            match forward(host, me, Peer::Realm, &reply).await? {
                Done::Done => writeln!(out, "replied/{question} 1")?,
                Done::Refused { why } => refused(question, &why)?,
            }
        }
        [verb, task @ ..] if verb == "may-spend" => {
            let task =
                task_or_standing(host, me, task.first()).await?;
            let may = may_spend(host, me, task).await?;
            writeln!(out, "may/{me} {}", i64::from(may))?;
        }
        [verb, call, micros, task @ ..] if verb == "charge" => {
            let task =
                task_or_standing(host, me, task.first()).await?;
            let micros = u32::try_from(number(Some(micros))?)?;
            charge(host, me, call, task, micros).await?;
            writeln!(out, "charged/{call} {micros}")?;
        }
        [verb, micros, times] if verb == "spend" => {
            let task = kept(host, me).await?.standing;
            let micros = u32::try_from(number(Some(micros))?)?;
            let mut spent = 0;
            for time in 1..=number(Some(times))? {
                if !may_spend(host, me, task.clone()).await? {
                    break;
                }
                let call = format!("spend-{time}");
                charge(host, me, &call, task.clone(), micros).await?;
                spent += u64::from(micros);
            }
            writeln!(out, "spent/{me} {spent}")?;
        }
        [verb, count, to] if verb == "letters" => {
            let count = number(Some(count))?;
            for letter in 1..=count {
                let call = format!("letter-{letter}");
                let text = format!("letter {letter}");
                if let Done::Refused { why } =
                    tell(host, me, &call, named(to)?, text).await?
                {
                    bail!("{call} was refused: {why}");
                }
            }
            writeln!(out, "told/{me} {count}")?;
        }
        [verb, call, script @ ..] if verb == "run" => {
            let (network, script) = match script {
                [net, script @ ..] if net == "net" => (true, script),
                _ => (false, script),
            };
            ensure!(!script.is_empty(), "run takes a script");
            let run = Run {
                call: CallId(call.clone()),
                conversation: ConversationId(CONVERSATION.to_owned()),
                realm_task: None,
                program: Program::Script {
                    script: script.join(" "),
                    network,
                    timeout_ms: None,
                },
            };
            match forward(host, me, Peer::Workspace, &run).await? {
                RunAnswer::Ended {
                    end: RunEnd::Ran(end),
                } => {
                    let (exit, code) = exit_word(&end.exit);
                    writeln!(out, "done/{call}/{exit} {code}")?;
                }
                RunAnswer::Ended {
                    end: RunEnd::NotRun { why },
                } => refused(call, &why)?,
                RunAnswer::Running { command } => {
                    writeln!(out, "running/{call} {}", command.0)?;
                }
                RunAnswer::WaitsForChief { needs } => {
                    writeln!(out, "waits/{call} {}", needs.len())?;
                }
                RunAnswer::Refused { why } => refused(call, &why)?,
            }
        }
        [verb] if verb == "inputs" => {
            let holding = kept(host, me).await?;
            writeln!(
                out,
                "standing/{me} {}",
                task_number(&holding.standing)
            )?;
            for input in holding.inputs {
                let task =
                    input.task.as_ref().map_or(-1, task_number);
                writeln!(
                    out,
                    "input/{me}/{}/{} {task}",
                    input.taken, input.kind
                )?;
                if input.kind == "question"
                    && let Some(letter) = input.letter
                {
                    writeln!(
                        out,
                        "question/{me}/{} {letter}",
                        input.taken
                    )?;
                }
            }
        }
        _ => bail!(
            "usage: demo agent <n> tell|ask|reply|may-spend|charge|\
             spend|letters|run|inputs ..."
        ),
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct Request<T> {
    id: String,
    act: T,
}

#[derive(serde::Serialize)]
struct Line {
    id: String,
    text: String,
    to: u64,
}

#[derive(serde::Serialize)]
struct Choice {
    choice: &'static str,
    id: String,
    wait: String,
}

async fn request<T: ChiefRequest>(
    host: &Host,
    act: T,
) -> Result<T::Answer> {
    let request = Request {
        id: fresh_id()?,
        act,
    };
    as_client(host, T::NAME, &request).await
}

async fn read_world(host: &Host) -> Result<Seen> {
    as_client(host, ReadWorld::NAME, &ReadWorld).await
}

async fn as_client<T: serde::de::DeserializeOwned>(
    host: &Host,
    name: &str,
    args: &impl serde::Serialize,
) -> Result<T> {
    let answer = host
        .call_for_client(
            &address::Realm.address()?,
            name,
            hamlet_actor::action::encode_positional(args)?,
            DRIVE_WITHIN,
        )
        .await?;
    Ok(ciborium::from_reader(answer.as_slice())?)
}

async fn forward<A: Action>(
    host: &Host,
    me: u32,
    to: Peer,
    action: &A,
) -> Result<A::Output> {
    let forward = Forward {
        to,
        name: A::NAME.to_owned(),
        args: hamlet_actor::action::encode_positional(action)?,
    };
    let reply = host
        .call(
            &address::Agent(AgentNumber(me)).address()?,
            Forward::NAME,
            hamlet_actor::action::encode_positional(&forward)?,
            DRIVE_WITHIN,
        )
        .await?;
    let answer =
        ciborium::from_reader::<Vec<u8>, _>(reply.as_slice())?;
    Ok(ciborium::from_reader(answer.as_slice())?)
}

async fn kept(host: &Host, me: u32) -> Result<Holding> {
    let reply = host
        .call(
            &address::Agent(AgentNumber(me)).address()?,
            Kept::NAME,
            hamlet_actor::action::encode_positional(&Kept)?,
            DRIVE_WITHIN,
        )
        .await?;
    Ok(ciborium::from_reader(reply.as_slice())?)
}

async fn tell(
    host: &Host,
    me: u32,
    call: &str,
    to: Named,
    text: String,
) -> Result<Done<AgentRefusal<LetterRefusal>>> {
    let tell = Tell {
        to,
        text,
        task: None,
        paths: Vec::new(),
    };
    forward(host, me, Peer::Realm, &tool(call, tell)).await
}

async fn may_spend(
    host: &Host,
    me: u32,
    task: RealmTask,
) -> Result<bool> {
    match forward(host, me, Peer::Realm, &MaySpend { task }).await? {
        Done::Done => Ok(true),
        Done::Refused { why } => {
            tracing::warn!(%why, "agent {me} may not spend");
            Ok(false)
        }
    }
}

async fn charge(
    host: &Host,
    me: u32,
    call: &str,
    task: RealmTask,
    micros: u32,
) -> Result<()> {
    let charge = Charge {
        call: CallId(call.to_owned()),
        task,
        micros,
        took_ms: 1,
        tokens: None,
    };
    forward(host, me, Peer::Realm, &charge).await
}

async fn task_or_standing(
    host: &Host,
    me: u32,
    task: Option<&String>,
) -> Result<RealmTask> {
    Ok(match task {
        Some(task) => RealmTask(task.clone()),
        None => kept(host, me).await?.standing,
    })
}

fn tool<T>(call: &str, act: T) -> Tool<T> {
    Tool {
        call: CallId(call.to_owned()),
        act,
    }
}

fn named(word: &str) -> Result<Named> {
    Ok(match word {
        "hand" => Named::Hand,
        "mayor" => Named::Mayor,
        _ => Named::Agent {
            agent: AgentNumber(agent_number(
                word.strip_prefix("hire#").unwrap_or(word),
            )?),
        },
    })
}

fn agent_number(word: &str) -> Result<u32> {
    Ok(u32::try_from(number(Some(&word.to_owned()))?)?)
}

fn task_number(task: &RealmTask) -> i64 {
    task.0.parse().unwrap_or(-1)
}

fn request_id(word: &str) -> String {
    let half = |seed: u64| {
        word.bytes().fold(seed, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    };
    let hex = format!(
        "{:016x}{:016x}",
        half(0xcbf2_9ce4_8422_2325),
        half(0x8422_2325_cbf2_9ce4)
    );
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn fresh_id() -> Result<String> {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    let made = MADE.fetch_add(1, Ordering::Relaxed);
    Ok(request_id(&format!(
        "{}-{}-{made}",
        now.as_nanos(),
        std::process::id()
    )))
}

fn now_ms() -> Result<i64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(i64::try_from(now.as_millis())?)
}

fn refused(name: &str, why: &impl std::fmt::Display) -> Result<()> {
    tracing::warn!(%why, "{name} was refused");
    writeln!(std::io::stdout(), "refused/{name} 1")?;
    Ok(())
}

fn reach_word(reach: &ReachSeen) -> (&'static str, u64) {
    match reach {
        ReachSeen::None => ("none", 0),
        ReachSeen::Applied { version } => ("applied", version.0),
        ReachSeen::Pending { wanted, .. } => ("pending", wanted.0),
    }
}

fn print_world(seen: &Seen) -> Result<()> {
    let mut out = std::io::stdout();
    writeln!(out, "world {}", seen.revision.0)?;
    for hamlet in &seen.hamlets {
        writeln!(out, "hamlet/{} {}", hamlet.name, hamlet.mayor.0)?;
    }
    for agent in &seen.agents {
        let n = agent.agent.0;
        let status = match agent.status {
            Status::Hiring => "hiring",
            Status::Live => "live",
            Status::Ended { .. } => "ended",
        };
        let lead = agent.lead.map_or(0, |lead| lead.0);
        writeln!(out, "agent/{n}/{status} {lead}")?;
        let (word, version) = reach_word(&agent.reach);
        writeln!(out, "reach/{n}/{word} {version}")?;
    }
    for wait in &seen.waits {
        writeln!(
            out,
            "wait/{}/{} {}",
            wait.agent.0, wait.call.0, wait.wait.0
        )?;
    }
    for spend in &seen.spend {
        let payer = match &spend.payer {
            Payer::Realm => "realm",
            Payer::Hamlet { hamlet } => seen
                .hamlets
                .iter()
                .find(|seen| seen.hamlet == *hamlet)
                .map_or(hamlet.0.as_str(), |seen| seen.name.as_str()),
        };
        writeln!(out, "spend/{payer} {}", spend.spent_micros)?;
        if let Some(budget) = spend.budget_micros {
            writeln!(out, "budget/{payer} {budget}")?;
        }
    }
    Ok(())
}
