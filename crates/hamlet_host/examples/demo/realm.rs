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
//! demo realm answer <wait> allow|always|refuse|go|withdraw|reply
//!     |dismiss [<text ...>]
//!                    answer the wait <wait> as the chief
//! demo realm end <n>
//!                    end agent <n> as the chief
//! demo realm budget <hamlet> <micros>
//!                    bound the hamlet <hamlet>'s spending
//! demo realm review <hamlet> always|remote|never
//! demo realm mode <hamlet> manual|auto
//! demo realm folder <hamlet> <path>
//!                    set the hamlet's review limit, mode or folder
//! demo realm allowance <n> <micros>
//!                    set hire <n>'s allowance
//! demo realm charter <text ...>
//! demo realm model <model>
//! demo realm price <model> <input> <output>
//!                    edit the charter, the realm's model, or the
//!                    model's price in dollars per million tokens
//! demo realm names <hamlet> <count>
//!                    rename the hamlet <count> times, timing each
//! demo realm live <n>
//!                    wait until agent <n> is live in its reach
//! demo realm world
//!                    read the world a client shows
//! demo realm reads <count> <ms> [<n>]
//!                    read the world <count> times, <ms> apart
//! demo agent <n> tell <call> <to> [task=<task>] [paths=<p>,...]
//!     <text ...>
//! demo agent <n> ask <call> <to> <text ...>
//!                    send a letter or a question as agent <n>'s call
//!                    <call>, to `hand`, `mayor` or `hire#<k>`
//! demo agent <n> reply <question> <text ...>
//!                    answer the question <question>
//! demo agent <n> ask-chief <call> <task> <question ...>
//! demo agent <n> object <call> <task> <reason ...>
//!                    ask the chief, or object to its line, on <task>
//! demo agent <n> start-worker <call> <brief ...>
//! demo agent <n> worker-done|errand-done <text ...>
//!                    start a worker, or end as a worker or an errand
//!                    with an answer, and leave
//! demo agent <n> agree <call> <mine> <theirs>
//!                    agree a road between two hires as a mayor
//! demo agent <n> may-spend [<task>]
//! demo agent <n> charge <call> <micros> [<task>]
//!                    ask whether agent <n> may spend, or charge its
//!                    model call <call>, for <task> or its standing task
//! demo agent <n> call <call> <tokens in> <tokens out> [<task>]
//!                    a model call priced from the model copy: ask,
//!                    then charge
//! demo agent <n> read-budget <call> [<task>]
//! demo agent <n> spend <micros> <times>
//!                    up to <times> times, ask and charge <micros>
//! demo agent <n> letters <count> <to>
//!                    tell <to> <count> letters, one after another
//! demo agent <n> run <call> [net] <script ...>
//!                    run a script as agent <n>'s call <call>, with
//!                    the network when `net` is given
//! demo agent <n> copies
//!                    read the realm's copies for agent <n>
//! demo agent <n> fetch
//!                    fetch the files of the parcels agent <n> was
//!                    given into its folder's `inbox/`
//! demo agent <n> keep-credential <provider> <key>
//! demo agent <n> inputs
//!                    list the inputs agent <n> took
//! demo agent <n> await <kind> <count>
//!                    wait until agent <n> took <count> inputs of
//!                    <kind>
//! demo agent <first>..<last> ...
//!                    run the command as each agent at once
//! demo ... &         run the command in the background
//! ```
//!
//! A `<task>` is a task's number, or `line:<k>`: the task of the
//! newest line agent `<k>` took. Each command prints `<name> <i64>`
//! lines: `founded/<name> <mayor>`, `hired/<call> <agent>`,
//! `taken/<id> <task>` or `not_taken/<id> <task>`, `answered/<wait>
//! 1`, `ended/<n> 1`, `budget/<hamlet> <micros>`, `<verb>/<name> 1`
//! for `review`, `mode`, `folder`, `allowance`, `charter`, `model`
//! and `price`, `names/p50 <us>` and `names/p95 <us>`, `live/<n> 1`
//! or `0`, `sent/<call> 1`, `replied/<question> 1`, `asked/<call> 1`,
//! `objected/<call> 1`, `started/<call> <worker>`, `<verb>/<n> 1` for
//! `worker-done` and `errand-done`, `waiting/<call> 1` or
//! `laid/<call> 1`, `may/<n> 1` or `0`, `charged/<call> <micros>`,
//! `called/<call> <micros>`, `own/<n> <micros>` and `allowance/<n>
//! <micros>` (`-1` for none), `spent/<n> <micros>`, `told/<n>
//! <count>`, `copy/<n>/<part> <version>`, `fetched/<n> <bytes>`,
//! `kept/<provider> 1`, `took/<n>/<kind> <count>`, and for `run`,
//! `done/<call>/<exit> <code>`, `running/<call> <command>` or
//! `waits/<call> <needs>`. A refusal prints `refused/<name> 1` and
//! logs why. `world` prints `world <revision>`, then `hamlet/<name>
//! <mayor>`, `agent/<n>/<status> <lead>`,
//! `reach/<n>/<none|applied|pending> <version>`, `wait/<n>/<call>
//! <wait>` for a tool's wait, and `spend/<payer> <micros>` with
//! `budget/<payer> <micros>` where a budget bounds it. `reads` prints
//! `read/<i> <us>`, the read's time, with `<n>`'s reach whenever it
//! changes, and then `reads/longest <us>`. `inputs` prints
//! `standing/<n> <task>`, then `input/<n>/<taken>/<kind> <task>` in
//! the order taken, and `question/<n>/<taken> <letter>` for a
//! question. The stand-in agent keeps its inputs with a line's note,
//! the files it was given, the copies the realm pushes, its newest
//! place and the ends the workspace tells it of in its file, keeps
//! each input it drops as a repeat of one it took in its `repeat`
//! table, logs that it was made, and prints
//! `finished/<call>/<exit> <code>` for each end.

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
use hamlet_messages::agent::chat::Count;
use hamlet_messages::agent::copies::{Copies, Price, Versioned};
use hamlet_messages::agent::{
    AgentInput, AgentKind, Fill, Finished, Input, Outcome, Place,
    Submit,
};
use hamlet_messages::profile::Branch;
use hamlet_messages::realm::budget::{
    BudgetLimit, BudgetRead, Charge, MaySpend, Payer, ReadBudget,
};
use hamlet_messages::realm::client::{
    self, AnswerRefusal, ReadWorld, Say, SayAnswer,
};
use hamlet_messages::realm::documents::{
    CardSetting, EditCatalogue, EditCharter, EditSettings,
    ReadCopies, Setting,
};
use hamlet_messages::realm::hamlets::{
    NameHamlet, SetFolder, SetMode,
};
use hamlet_messages::realm::models::{self, KeepCredential};
use hamlet_messages::realm::oversight::{AskChief, Object, Question};
use hamlet_messages::realm::parcels::{
    FetchPart, ParcelFile, ParcelId, Part,
};
use hamlet_messages::realm::workers::{
    Ended, ErrandDone, SetAllowance, StartWorker, Started, WorkerDone,
};
use hamlet_messages::realm::world::{
    ReachSeen, Seen, Status, WaitKind,
};
use hamlet_messages::realm::{
    AgentRefusal, AgreeRoad, Agreed, Ask, ChiefRequest, Done,
    Enclosed, End, Found, FoundAnswer, HamletId, Hire, HireAnswer,
    LetterId, LetterRefusal, Limit, Named, NeedsChief, Reply, Review,
    SecretList, Secrets, SetLimits, Tell, Tier, Tool,
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
     TEXT, sender INTEGER, letter INTEGER, note TEXT, at INTEGER \
     NOT NULL) STRICT; CREATE TABLE IF NOT EXISTS finished (call \
     TEXT NOT NULL, exit TEXT NOT NULL, code INTEGER NOT NULL, at \
     INTEGER NOT NULL) STRICT; CREATE TABLE IF NOT EXISTS given \
     (parcel INTEGER NOT NULL, file INTEGER NOT NULL, path TEXT NOT \
     NULL, size INTEGER NOT NULL, fetched_at INTEGER, PRIMARY KEY \
     (parcel, file)) STRICT; CREATE TABLE IF NOT EXISTS copy (part \
     TEXT PRIMARY KEY, version INTEGER NOT NULL, value TEXT NOT \
     NULL, at INTEGER NOT NULL) STRICT; CREATE TABLE IF NOT EXISTS \
     repeat (id TEXT NOT NULL, kind TEXT NOT NULL, at INTEGER NOT \
     NULL) STRICT;";

#[async_trait::async_trait]
impl Actor for StandInAgent {
    type Input = Option<AgentInput>;
    type Actions = (
        Submit,
        Place,
        Fill,
        Finished,
        Copies,
        Forward,
        Kept,
        Fetch,
        Leave,
    );
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
        tracing::info!(
            task = input.task.0,
            "{} was made, a {kind}",
            ctx.actor_id()
        );
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
        let none: &[ParcelFile] = &[];
        let (kind, text, from, letter, files) = match &submit.input {
            Input::Line { text, .. } => {
                ("line", text, None, None, none)
            }
            Input::Notice { text, .. } => {
                ("notice", text, None, None, none)
            }
            Input::WorkerAnswer { text, .. } => {
                ("worker_answer", text, None, None, none)
            }
            Input::Letter { from, text, files } => {
                ("letter", text, Some(from), None, files.as_slice())
            }
            Input::Question {
                letter,
                from,
                text,
                files,
            } => (
                "question",
                text,
                Some(from),
                Some(letter),
                files.as_slice(),
            ),
            Input::Answer {
                question,
                from,
                text,
                ..
            } => ("answer", text, Some(from), Some(question), none),
        };
        let note = match &submit.input {
            Input::Line {
                note: Some(note), ..
            } => BindParam::Text(note.clone()),
            _ => BindParam::Null,
        };
        for given in files {
            ctx.sql()
                .execute(
                    "INSERT INTO given (parcel, file, path, size) \
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
                    Some(vec![
                        BindParam::Integer(i64::from(given.parcel.0)),
                        BindParam::Integer(i64::from(given.file)),
                        BindParam::Text(
                            given.path.to_string_lossy().into_owned(),
                        ),
                        BindParam::Integer(i64::try_from(
                            given.size,
                        )?),
                    ]),
                )
                .await?;
        }
        let kept = ctx
            .sql()
            .execute(
                "INSERT INTO input (id, kind, text, task, sender, \
                 letter, note, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, \
                 ?7, ?8) ON CONFLICT (id) DO NOTHING",
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
                    note,
                    BindParam::Integer(now_ms()?),
                ]),
            )
            .await?;
        let repeat = kept.changes == 0;
        if repeat {
            ctx.sql()
                .execute(
                    "INSERT INTO repeat (id, kind, at) VALUES (?1, \
                     ?2, ?3)",
                    Some(vec![
                        BindParam::Text(submit.id.0.clone()),
                        BindParam::Text(kind.to_owned()),
                        BindParam::Integer(now_ms()?),
                    ]),
                )
                .await?;
        }
        tracing::info!(
            id = submit.id.0,
            kind,
            text,
            files = files.len(),
            repeat,
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
        std::fs::create_dir_all(agent_folder(&ctx)?)?;
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

impl Handler<Copies> for StandInAgent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        copies: Copies,
    ) -> Result<()> {
        sender(&ctx, &copies)?;
        for (part, version, value) in copy_parts(&copies) {
            tracing::info!(
                version,
                value,
                "{} took the copy of its {part}",
                ctx.actor_id()
            );
            ctx.sql()
                .execute(
                    "INSERT INTO copy (part, version, value, at) \
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT (part) DO \
                     UPDATE SET version = excluded.version, value = \
                     excluded.value, at = excluded.at",
                    Some(vec![
                        BindParam::Text(part.to_owned()),
                        BindParam::Integer(i64::from(version)),
                        BindParam::Text(value),
                        BindParam::Integer(now_ms()?),
                    ]),
                )
                .await?;
        }
        Ok(())
    }
}

/// Each part `copies` holds: its name, version and value as shown.
fn copy_parts(copies: &Copies) -> Vec<(&'static str, u32, String)> {
    fn part<T: std::fmt::Debug>(
        name: &'static str,
        copy: Option<&Versioned<T>>,
    ) -> Option<(&'static str, u32, String)> {
        copy.map(|copy| {
            (name, copy.version.0, format!("{:?}", copy.value))
        })
    }
    [
        part("charter", copies.charter.as_ref()),
        part("character", copies.character.as_ref()),
        part("model", copies.model.as_ref()),
        part("settings", copies.settings.as_ref()),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Fetches the files of the parcels the agent was given and has not
/// fetched, part by part, into its folder's `inbox/`.
#[derive(serde::Serialize, serde::Deserialize)]
struct Fetch;

impl Action for Fetch {
    type Output = u64;
    const NAME: &'static str = "fetch";
}

impl Handler<Fetch> for StandInAgent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Fetch,
    ) -> Result<u64> {
        let host = HostRef::of(ctx.inner())?;
        let inbox = agent_folder(&ctx)?.join("inbox");
        let given = ctx
            .sql()
            .query(
                "SELECT parcel, file, path FROM given WHERE \
                 fetched_at IS NULL ORDER BY parcel, file",
                None,
            )
            .await?
            .rows;
        let mut bytes = 0;
        for row in given {
            let [
                ColumnValue::Integer(parcel),
                ColumnValue::Integer(file),
                path,
            ] = row.as_slice()
            else {
                bail!("a given file's row reads as it was written");
            };
            let path = inbox.join(text(path).context("a path")?);
            let partial = path.with_extension("part");
            std::fs::create_dir_all(
                path.parent().context("a file has a folder")?,
            )?;
            let mut out = std::fs::File::create(&partial)?;
            for part in 0.. {
                let fetch = FetchPart {
                    parcel: ParcelId(u32::try_from(*parcel)?),
                    file: u32::try_from(*file)?,
                    part,
                };
                match call(
                    &host,
                    &address::Realm,
                    &fetch,
                    DRIVE_WITHIN,
                )
                .await?
                {
                    Part::Bytes { bytes: got, last } => {
                        out.write_all(&got.0)?;
                        bytes += u64::try_from(got.0.len())?;
                        if last {
                            break;
                        }
                    }
                    Part::Refused { why } => {
                        bail!("part {part} was refused: {why}")
                    }
                }
            }
            out.sync_all()?;
            std::fs::rename(&partial, &path)?;
            ctx.sql()
                .execute(
                    "UPDATE given SET fetched_at = ?3 WHERE parcel \
                     = ?1 AND file = ?2",
                    Some(vec![
                        BindParam::Integer(*parcel),
                        BindParam::Integer(*file),
                        BindParam::Integer(now_ms()?),
                    ]),
                )
                .await?;
        }
        Ok(bytes)
    }
}

/// Ends the agent's actor, as a worker or an errand does once it has
/// said it is done.
#[derive(serde::Serialize, serde::Deserialize)]
struct Leave;

impl Action for Leave {
    type Output = ();
    const NAME: &'static str = "leave";
}

impl Handler<Leave> for StandInAgent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Leave,
    ) -> Result<()> {
        ctx.destroy()
    }
}

fn agent_folder(
    ctx: &Ctx<StandInAgent>,
) -> Result<std::path::PathBuf> {
    let Some(ActorKeySegment::String(key)) = ctx.key().first() else {
        bail!("an agent's key is its number");
    };
    Ok(HostRef::of(ctx.inner())?
        .folder()
        .join(format!("actors/agent/{key}.fs")))
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

/// A fetch of a large parcel takes many calls.
const FETCH_WITHIN: Duration = Duration::from_secs(600);

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
        [verb, wait, choice, text @ ..] if verb == "answer" => {
            let choice = Choice {
                choice: match choice.as_str() {
                    "allow" => "allow",
                    "always" => "always",
                    "refuse" => "deny",
                    "go" => "go",
                    "withdraw" => "withdraw",
                    "reply" => "reply",
                    "dismiss" => "dismiss",
                    _ => bail!(
                        "an answer is allow, always, refuse, go, \
                         withdraw, reply or dismiss"
                    ),
                },
                id: fresh_id()?,
                text: (!text.is_empty()).then(|| text.join(" ")),
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
            let limits = SetLimits {
                hamlet: Some(hamlet_named(host, name).await?),
                budget: vec![BudgetLimit::Cost {
                    micros: number(Some(micros))?,
                }],
                limits: Vec::new(),
            };
            match request(host, limits).await? {
                Done::Done => {
                    writeln!(out, "budget/{name} {micros}")?;
                }
                Done::Refused { why } => refused(name, &why)?,
            }
        }
        [verb, name, review] if verb == "review" => {
            let limits = SetLimits {
                hamlet: Some(hamlet_named(host, name).await?),
                budget: Vec::new(),
                limits: vec![Limit::Review {
                    review: match review.as_str() {
                        "always" => Review::Always,
                        "remote" => Review::Remote,
                        "never" => Review::Never,
                        _ => {
                            bail!("review is always, remote or never")
                        }
                    },
                }],
            };
            done("review", name, request(host, limits).await?)?;
        }
        [verb, name, mode] if verb == "mode" => {
            let mode = SetMode {
                hamlet: hamlet_named(host, name).await?,
                mode: match mode.as_str() {
                    "manual" => Mode::Manual,
                    "auto" => Mode::Auto,
                    _ => bail!("a mode is manual or auto"),
                },
            };
            done("mode", name, request(host, mode).await?)?;
        }
        [verb, name, folder] if verb == "folder" => {
            let folder = SetFolder {
                hamlet: hamlet_named(host, name).await?,
                folder: Some(folder.into()),
            };
            done("folder", name, request(host, folder).await?)?;
        }
        [verb, name, count] if verb == "names" => {
            let hamlet = hamlet_named(host, name).await?;
            let mut took = Vec::new();
            for time in 1..=number(Some(count))? {
                let begun = Instant::now();
                let named = NameHamlet {
                    hamlet: hamlet.clone(),
                    name: format!("{name}-{time}"),
                };
                if let Done::Refused { why } =
                    request(host, named).await?
                {
                    bail!("naming {name} was refused: {why}");
                }
                took.push(begun.elapsed().as_micros());
            }
            took.sort_unstable();
            let at =
                |share: usize| took[(took.len() - 1) * share / 100];
            writeln!(out, "names/p50 {}", at(50))?;
            writeln!(out, "names/p95 {}", at(95))?;
        }
        [verb, agent, micros] if verb == "allowance" => {
            let allowance = SetAllowance {
                hire: Named::Agent {
                    agent: AgentNumber(agent_number(agent)?),
                },
                micros: Some(Count(number(Some(micros))?)),
                why: None,
            };
            done(
                "allowance",
                agent,
                request(host, allowance).await?,
            )?;
        }
        [verb, text @ ..]
            if verb == "charter" && !text.is_empty() =>
        {
            let charter = EditCharter {
                text: Some(text.join(" ")),
            };
            done("charter", "realm", request(host, charter).await?)?;
        }
        [verb, model] if verb == "model" => {
            let settings = EditSettings {
                set: vec![Setting::Model {
                    model: model.clone(),
                }],
            };
            done("model", model, request(host, settings).await?)?;
        }
        [verb, model, input, output] if verb == "price" => {
            let catalogue = EditCatalogue {
                model: model.clone(),
                set: vec![CardSetting::Price {
                    price: Some(Price {
                        input: input.parse()?,
                        output: output.parse()?,
                        cache_read: 0.0,
                        cache_write: 0.0,
                    }),
                }],
            };
            done("price", model, request(host, catalogue).await?)?;
        }
        [verb, agent] if verb == "live" => {
            let agent = agent_number(agent)?;
            let live = until(move || async move {
                let seen = read_world(host).await?;
                Ok(seen
                    .agents
                    .iter()
                    .any(|seen| {
                        seen.agent.0 == agent
                            && matches!(seen.status, Status::Live)
                            && matches!(
                                seen.reach,
                                ReachSeen::Applied { .. }
                            )
                    })
                    .then_some(()))
            })
            .await?;
            writeln!(
                out,
                "live/{agent} {}",
                i64::from(live.is_some())
            )?;
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
            "usage: demo realm found|hire|say|answer|end|budget|\
             review|mode|folder|allowance|charter|model|price|names|\
             live|world|reads ..."
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
        [verb, call, to, words @ ..] if verb == "tell" => {
            let mut tell = Tell {
                to: named(to)?,
                text: String::new(),
                task: None,
                paths: Vec::new(),
            };
            let mut text = words;
            while let [word, rest @ ..] = text {
                match word.split_once('=') {
                    Some(("task", task)) => {
                        tell.task =
                            Some(task_word(host, task).await?);
                    }
                    Some(("paths", paths)) => {
                        tell.paths = paths
                            .split(',')
                            .map(|path| Enclosed {
                                path: path.into(),
                            })
                            .collect();
                    }
                    _ => break,
                }
                text = rest;
            }
            tell.text = text.join(" ");
            match forward(host, me, Peer::Realm, &tool(call, tell))
                .await?
            {
                Done::Done => writeln!(out, "sent/{call} 1")?,
                Done::Refused { why } => refused(call, &why)?,
            }
        }
        [verb, call, task, question @ ..]
            if verb == "ask-chief" && !question.is_empty() =>
        {
            let ask = AskChief {
                task: task_word(host, task).await?,
                conversation: ConversationId(CONVERSATION.to_owned()),
                message: None,
                questions: vec![Question {
                    question: question.join(" "),
                    description: None,
                    options: Vec::new(),
                    text_input: true,
                }],
            };
            done(
                "asked",
                call,
                forward(host, me, Peer::Realm, &tool(call, ask))
                    .await?,
            )?;
        }
        [verb, call, task, reason @ ..] if verb == "object" => {
            let object = Object {
                task: task_word(host, task).await?,
                conversation: ConversationId(CONVERSATION.to_owned()),
                reason: reason.join(" "),
            };
            done(
                "objected",
                call,
                forward(host, me, Peer::Realm, &tool(call, object))
                    .await?,
            )?;
        }
        [verb, call, brief @ ..] if verb == "start-worker" => {
            let start = StartWorker {
                brief: brief.join(" "),
                tools: Vec::new(),
                task: None,
                hamlet: None,
            };
            match forward(host, me, Peer::Realm, &tool(call, start))
                .await?
            {
                Started::Started { worker } => {
                    writeln!(out, "started/{call} {}", worker.0)?;
                }
                Started::Refused { why } => refused(call, &why)?,
            }
        }
        [verb, text @ ..]
            if verb == "worker-done" || verb == "errand-done" =>
        {
            let ended = Ended::Answered {
                text: text.join(" "),
            };
            if verb == "worker-done" {
                forward(host, me, Peer::Realm, &WorkerDone { ended })
                    .await?;
            } else {
                forward(host, me, Peer::Realm, &ErrandDone { ended })
                    .await?;
            }
            at_agent(host, me, &Leave, DRIVE_WITHIN).await?;
            writeln!(out, "{verb}/{me} 1")?;
        }
        [verb, call, tokens_in, tokens_out, task @ ..]
            if verb == "call" =>
        {
            let price = read_copies(host, me)
                .await?
                .model
                .and_then(|model| model.value.price)
                .context("the agent's model copy holds no price")?;
            let tokens_in = u32::try_from(number(Some(tokens_in))?)?;
            let tokens_out =
                u32::try_from(number(Some(tokens_out))?)?;
            let micros = f64::from(tokens_in) * price.input
                + f64::from(tokens_out) * price.output;
            ensure!(
                (0.0..=f64::from(u32::MAX)).contains(&micros),
                "a call costs {micros} millionths of a dollar"
            );
            let micros = micros.round() as u32;
            let task =
                task_or_standing(host, me, task.first()).await?;
            if may_spend(host, me, task.clone()).await? {
                let tokens = Some(tokens_in + tokens_out);
                charge(host, me, call, task, micros, tokens).await?;
                writeln!(out, "called/{call} {micros}")?;
            } else {
                writeln!(out, "refused/{call} 1")?;
            }
        }
        [verb, call, task @ ..] if verb == "read-budget" => {
            let task = match task.first() {
                Some(task) => Some(task_word(host, task).await?),
                None => None,
            };
            let read = ReadBudget { task };
            match forward(host, me, Peer::Realm, &tool(call, read))
                .await?
            {
                BudgetRead::Budget {
                    spend,
                    own_micros,
                    allowance_micros,
                    ..
                } => {
                    tracing::info!(
                        ?spend,
                        own_micros,
                        ?allowance_micros,
                        "agent {me} read its budget"
                    );
                    writeln!(out, "own/{me} {own_micros}")?;
                    writeln!(
                        out,
                        "allowance/{me} {}",
                        allowance_micros
                            .map_or(Ok(-1), i64::try_from)?
                    )?;
                }
                BudgetRead::Refused { why } => refused(call, &why)?,
            }
        }
        [verb] if verb == "copies" => {
            for (part, version, value) in
                copy_parts(&read_copies(host, me).await?)
            {
                tracing::info!(
                    version,
                    value,
                    "agent {me} read the copy of its {part}"
                );
                writeln!(out, "copy/{me}/{part} {version}")?;
            }
        }
        [verb] if verb == "fetch" => {
            let bytes =
                at_agent(host, me, &Fetch, FETCH_WITHIN).await?;
            writeln!(out, "fetched/{me} {bytes}")?;
        }
        [verb, provider, key] if verb == "keep-credential" => {
            let keep = KeepCredential {
                provider: provider.clone(),
                value: models::Kept::Key { key: key.clone() },
            };
            forward(host, me, Peer::Realm, &keep).await?;
            writeln!(out, "kept/{provider} 1")?;
        }
        [verb, call, mine, theirs] if verb == "agree" => {
            let agree = AgreeRoad {
                mine: named(mine)?,
                theirs: named(theirs)?,
                why: None,
            };
            match forward(host, me, Peer::Realm, &tool(call, agree))
                .await?
            {
                Agreed::Waiting => writeln!(out, "waiting/{call} 1")?,
                Agreed::Laid => writeln!(out, "laid/{call} 1")?,
                Agreed::Refused { why } => refused(call, &why)?,
            }
        }
        [verb, kind, count] if verb == "await" => {
            let count = usize::try_from(number(Some(count))?)?;
            let took = move || async move {
                let holding = kept(host, me).await?;
                anyhow::Ok(
                    holding
                        .inputs
                        .iter()
                        .filter(|input| input.kind == *kind)
                        .count(),
                )
            };
            until(move || async move {
                Ok((took().await? >= count).then_some(()))
            })
            .await?;
            writeln!(out, "took/{me}/{kind} {}", took().await?)?;
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
            charge(host, me, call, task, micros, None).await?;
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
                charge(host, me, &call, task.clone(), micros, None)
                    .await?;
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
            "usage: demo agent <n> tell|ask|reply|ask-chief|object|\
             start-worker|worker-done|errand-done|agree|may-spend|\
             charge|call|read-budget|spend|letters|run|copies|fetch|\
             keep-credential|inputs|await ..."
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
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
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

async fn hamlet_named(host: &Host, name: &str) -> Result<HamletId> {
    read_world(host)
        .await?
        .hamlets
        .into_iter()
        .find(|hamlet| hamlet.name == name)
        .map(|hamlet| hamlet.hamlet)
        .with_context(|| format!("no hamlet is named `{name}`"))
}

/// Calls `look` every 50 ms until it finds something, for at most
/// [`DRIVE_WITHIN`].
async fn until<T, F: Future<Output = Result<Option<T>>>>(
    look: impl Fn() -> F,
) -> Result<Option<T>> {
    let end = Instant::now() + DRIVE_WITHIN;
    loop {
        if let Some(found) = look().await? {
            return Ok(Some(found));
        }
        if Instant::now() >= end {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The task a word names: `line:<n>` for the task of the newest line
/// agent `<n>` took, waiting for one, or else the task's own name.
async fn task_word(host: &Host, word: &str) -> Result<RealmTask> {
    let Some(agent) = word.strip_prefix("line:") else {
        return Ok(RealmTask(word.to_owned()));
    };
    let agent = agent_number(agent)?;
    until(move || async move {
        let holding = kept(host, agent).await?;
        Ok(holding
            .inputs
            .into_iter()
            .rev()
            .find(|input| input.kind == "line")
            .and_then(|input| input.task))
    })
    .await?
    .with_context(|| format!("agent {agent} took no line"))
}

/// Prints `<verb>/<name> 1`, or the refusal.
fn done<W: std::fmt::Display>(
    verb: &str,
    name: &str,
    done: Done<W>,
) -> Result<()> {
    match done {
        Done::Done => writeln!(std::io::stdout(), "{verb}/{name} 1")?,
        Done::Refused { why } => refused(name, &why)?,
    }
    Ok(())
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
    let answer = at_agent(host, me, &forward, DRIVE_WITHIN).await?;
    Ok(ciborium::from_reader(answer.as_slice())?)
}

async fn kept(host: &Host, me: u32) -> Result<Holding> {
    at_agent(host, me, &Kept, DRIVE_WITHIN).await
}

async fn read_copies(host: &Host, me: u32) -> Result<Copies> {
    let have = ReadCopies {
        charter: None,
        character: None,
        model: None,
        settings: None,
    };
    forward(host, me, Peer::Realm, &have).await
}

/// Runs a stand-in agent's own action on agent `me`.
async fn at_agent<A: Action>(
    host: &Host,
    me: u32,
    action: &A,
    within: Duration,
) -> Result<A::Output> {
    let reply = host
        .call(
            &address::Agent(AgentNumber(me)).address()?,
            A::NAME,
            hamlet_actor::action::encode_positional(action)?,
            within,
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
    tokens: Option<u32>,
) -> Result<()> {
    let charge = Charge {
        call: CallId(call.to_owned()),
        task,
        micros,
        took_ms: 1,
        tokens,
    };
    forward(host, me, Peer::Realm, &charge).await
}

async fn task_or_standing(
    host: &Host,
    me: u32,
    task: Option<&String>,
) -> Result<RealmTask> {
    match task {
        Some(task) => task_word(host, task).await,
        None => Ok(kept(host, me).await?.standing),
    }
}

fn tool<T>(call: &str, act: T) -> Tool<T> {
    Tool {
        call: CallId(call.to_owned()),
        task: None,
        conversation: None,
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
        let WaitKind::Tool { call, .. } = &wait.kind else {
            continue;
        };
        writeln!(
            out,
            "wait/{}/{} {}",
            wait.agent.0, call.0, wait.wait.0
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
