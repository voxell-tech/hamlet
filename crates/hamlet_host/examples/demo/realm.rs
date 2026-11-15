//! A stand-in for the realm: the actor `realm/local`, which sets
//! agents' reach on the workspace by calling it as itself, answers
//! the workspace's call for its secrets with one made-up secret,
//! `demo-secret-value`, of version 1, and keeps each call the
//! workspace says waits for a chief in its `waits` table. For the
//! linked demo it calls any actor's action as itself (`tell` in
//! demo/link.rs), answers who calls it and powers of two to anyone,
//! and keeps in its `resumed` table how many jobs each agent says it
//! finished after a kill.
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

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hamlet_actor::action::encode_positional;
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{
    ActorConfig, ActorFactory, BindParam, ColumnValue,
};
use hamlet_host::{Address, Caller, Host, HostRef};
use hamlet_messages::Message;
use hamlet_messages::address::{self, Party};
use hamlet_messages::realm::{NeedsChief, SecretList, Secrets};
use hamlet_messages::sender::{Sender, sender, wrong_caller};
use hamlet_messages::workspace::reach::{
    Footing, Mode, Project, Reach, SecretsVersion, Trust,
};
use hamlet_messages::workspace::{
    AgentNumber, Answer, AnswerTaken, CallId, ChiefAnswer,
    ConversationId, InForce, Lapse, ReachVersion, SetReach,
};

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

/// Asks the realm to call the action `name` of the actor at `to` with
/// `args` as itself, and to answer what it answers.
/// The program's own action, so it checks no sender.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Tell {
    pub to: Address,
    pub name: String,
    pub args: Vec<u8>,
}

impl Action for Tell {
    /// The other actor's answer, as it encoded it.
    type Output = Vec<u8>;
    const NAME: &'static str = "tell";
}

/// Asks the realm who it reads as this call's caller. Any caller may
/// ask.
#[derive(serde::Serialize, serde::Deserialize)]
struct Who;

impl Action for Who {
    type Output = Option<Caller>;
    const NAME: &'static str = "who";
}

/// Asks the realm for 2 to the power of this, below 64. Any caller
/// may ask.
#[derive(serde::Serialize, serde::Deserialize)]
struct Power(u8);

impl Action for Power {
    type Output = u64;
    const NAME: &'static str = "power";
}

/// An agent says it finished this many jobs a kill cut off. Only an
/// agent may send it, about itself.
#[derive(serde::Serialize, serde::Deserialize)]
struct Resumed(u32);

impl Action for Resumed {
    type Output = ();
    const NAME: &'static str = "resumed";
}

/// Asks the realm how many jobs the agent said it resumed, in all.
/// The program's own action, so it checks no sender.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Heard(pub AgentNumber);

impl Action for Heard {
    type Output = u64;
    const NAME: &'static str = "heard";
}

const TELL_WITHIN: Duration = Duration::from_secs(30);

/// How long the program's call of the realm may take: longer than
/// any call the realm makes for it.
const ASK_WITHIN: Duration = Duration::from_secs(60);

#[async_trait::async_trait]
impl Actor for Realm {
    type Input = ();
    type Actions = (
        Grant,
        Decide,
        Secrets,
        NeedsChief,
        Tell,
        Who,
        Power,
        Resumed,
        Heard,
    );
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
        ctx.sql()
            .exec(
                "CREATE TABLE IF NOT EXISTS resumed (agent INTEGER \
                 NOT NULL, jobs INTEGER NOT NULL) STRICT",
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

impl Handler<Tell> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        tell: Tell,
    ) -> Result<Vec<u8>> {
        let answer = HostRef::of(ctx.inner())?
            .call(&tell.to, &tell.name, tell.args, TELL_WITHIN)
            .await?;
        hamlet_host::fault::point(&format!("told/{}", tell.name));
        Ok(answer)
    }
}

impl Handler<Who> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Who,
    ) -> Result<Option<Caller>> {
        let caller = caller(&ctx)?;
        tracing::info!(?caller, "the realm was asked who calls");
        Ok(caller)
    }
}

impl Handler<Power> for Realm {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        Power(bits): Power,
    ) -> Result<u64> {
        1_u64
            .checked_shl(u32::from(bits))
            .context("power takes a number below 64")
    }
}

impl Handler<Resumed> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Resumed(jobs): Resumed,
    ) -> Result<()> {
        let caller = caller(&ctx)?;
        let Some(address::Agent(agent)) =
            address::Agent::of(caller.as_ref())
        else {
            return Err(wrong_caller(Resumed::NAME, caller));
        };
        ctx.sql()
            .execute(
                "INSERT INTO resumed (agent, jobs) VALUES (?1, ?2)",
                Some(vec![
                    BindParam::Integer(i64::from(agent.0)),
                    BindParam::Integer(i64::from(jobs)),
                ]),
            )
            .await?;
        tracing::info!("agent/{} resumed {jobs} jobs", agent.0);
        Ok(())
    }
}

impl Handler<Heard> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Heard(agent): Heard,
    ) -> Result<u64> {
        let heard = ctx
            .sql()
            .query(
                "SELECT COALESCE(SUM(jobs), 0) FROM resumed WHERE \
                 agent = ?1",
                Some(vec![BindParam::Integer(i64::from(agent.0))]),
            )
            .await?;
        match heard.rows.first().and_then(|row| row.first()) {
            Some(ColumnValue::Integer(jobs)) => {
                Ok(u64::try_from(*jobs)?)
            }
            other => bail!("the jobs heard of read {other:?}"),
        }
    }
}

/// The caller the host stamped on the call `ctx` handles.
fn caller(ctx: &Ctx<Realm>) -> Result<Option<Caller>> {
    ctx.conn().context("the call has no connection")?.params()
}

async fn call<M: Message>(
    host: &HostRef,
    to: &M::To,
    message: &M,
    deadline: Duration,
) -> Result<M::Output> {
    let args = encode_positional(message)?;
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

/// Starts the realm, made if it has none, and asks it `action` as
/// the program.
pub async fn ask<A: Action>(
    host: &Host,
    action: &A,
) -> Result<A::Output> {
    let realm = address::Realm.address()?;
    host.get_or_start(&realm).await?;
    let reply = host
        .call(&realm, A::NAME, encode_positional(action)?, ASK_WITHIN)
        .await?;
    Ok(ciborium::from_reader(reply.as_slice())?)
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
    let in_force = ask(host, &Grant(set)).await?;
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
    let mut out = std::io::stdout();
    match ask(host, &decide).await? {
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
