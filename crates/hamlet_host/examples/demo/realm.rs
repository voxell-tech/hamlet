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
//! demo realm answer <n> <call> once|conversation|always|refuse
//!     |refuse=<reason>|stopped|rewound|ended
//!                    answer agent <n>'s call <call> as a chief, or
//!                    end its wait
//! ```
//!
//! `reach` prints `reach/<n> <v>`, the version in force the workspace
//! answered with, or `reach/<n> refused: <why>`. A wait the workspace
//! tells of prints `waits/<n>/<call> <needs>`, how many things it
//! needs, and logs its subject. `answer` prints `answered/<call> 1`
//! when the call runs, and else `answered/<call>/not_run 0`,
//! `answered/<call>/no_wait 0` or `answered/<call>/refused 0`,
//! logging why.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory, BindParam};
use hamlet_actors::workspace;
use hamlet_host::{Address, Caller, Cause, Host};
use hamlet_messages::realm::{NeedsChief, SecretList, Secrets};
use hamlet_messages::workspace::reach::{
    Footing, Mode, Project, Reach, SecretsVersion, Trust,
};
use hamlet_messages::workspace::{
    AgentNumber, Answer, AnswerTaken, CallId, ChiefAnswer,
    ConversationId, Lapse, ReachAnswer, ReachVersion, Scope,
    SetReach,
};

/// The kind `realm`.
pub fn factory() -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<Realm>(
        workspace::REALM,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry
        .into_factories()
        .remove(workspace::REALM)
        .expect("the registry holds the kind it registered")
}

struct Realm;

/// Asks the realm to set an agent's reach on the workspace.
#[derive(serde::Serialize, serde::Deserialize)]
struct Grant(SetReach);

impl Action for Grant {
    type Output = ReachAnswer;
    const NAME: &'static str = "grant";
}

/// Asks the realm to answer a call that waits for a chief.
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
    ) -> Result<ReachAnswer> {
        let reply = Host::call_from(
            ctx.inner(),
            &Address::new(workspace::KIND, workspace::MACHINE),
            SetReach::NAME,
            hamlet_actor::action::encode_positional(&set)?,
            Duration::from_secs(10),
            Cause::root("realm"),
        )
        .await?;
        Ok(ciborium::from_reader(reply.as_slice())?)
    }
}

impl Handler<Decide> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Decide(answer): Decide,
    ) -> Result<AnswerTaken> {
        let reply = Host::call_from(
            ctx.inner(),
            &Address::new(workspace::KIND, workspace::MACHINE),
            ChiefAnswer::NAME,
            hamlet_actor::action::encode_positional(&answer)?,
            Duration::from_secs(10),
            Cause::root("realm"),
        )
        .await?;
        Ok(ciborium::from_reader(reply.as_slice())?)
    }
}

impl Handler<NeedsChief> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: NeedsChief,
    ) -> Result<()> {
        let needs = i64::try_from(asked.needs.len())?;
        ctx.sql()
            .execute(
                "INSERT INTO waits (agent, call, needs, subject) \
                 VALUES (?1, ?2, ?3, ?4)",
                Some(vec![
                    BindParam::Integer(i64::try_from(asked.agent.0)?),
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
        _: Ctx<Self>,
        _: Secrets,
    ) -> Result<SecretList> {
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
    host.get_or_start(workspace::KIND, workspace::MACHINE)
        .await?;
    host.get_or_start(workspace::REALM, workspace::REALM_KEY)
        .await?;
    let reply = host
        .call(
            &Address::new(workspace::REALM, workspace::REALM_KEY),
            Grant::NAME,
            hamlet_actor::action::encode_positional(&Grant(set))?,
            Duration::from_secs(20),
            Cause::root("demo"),
        )
        .await?;
    let mut out = std::io::stdout();
    match ciborium::from_reader(reply.as_slice())? {
        ReachAnswer::InForce(in_force) => {
            writeln!(out, "reach/{agent} {}", in_force.0)?;
        }
        ReachAnswer::Refused(refusal) => {
            writeln!(out, "reach/{agent} refused: {refusal}")?;
        }
    }
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
            "once" => Answer::Allow(Scope::Once),
            "conversation" => Answer::Allow(Scope::Conversation),
            "always" => Answer::Allow(Scope::Always),
            "refuse" => Answer::Refuse { reason: None },
            "stopped" => Answer::Lapsed(Lapse::Stopped),
            "rewound" => Answer::Lapsed(Lapse::Rewound),
            "ended" => Answer::Lapsed(Lapse::Ended),
            _ => bail!("`{answer}` is not an answer"),
        },
    };
    let decide = Decide(ChiefAnswer {
        agent: AgentNumber(agent.parse()?),
        call: CallId(call.to_owned()),
        answer,
    });
    host.get_or_start(workspace::KIND, workspace::MACHINE)
        .await?;
    host.get_or_start(workspace::REALM, workspace::REALM_KEY)
        .await?;
    let reply = host
        .call(
            &Address::new(workspace::REALM, workspace::REALM_KEY),
            Decide::NAME,
            hamlet_actor::action::encode_positional(&decide)?,
            Duration::from_secs(20),
            Cause::root("demo"),
        )
        .await?;
    let mut out = std::io::stdout();
    match ciborium::from_reader(reply.as_slice())? {
        AnswerTaken::Runs => writeln!(out, "answered/{call} 1")?,
        AnswerTaken::NotRun(why) => {
            tracing::warn!(%why, "agent {agent}'s call {call} did not run");
            writeln!(out, "answered/{call}/not_run 0")?;
        }
        AnswerTaken::NoWait => {
            writeln!(out, "answered/{call}/no_wait 0")?;
        }
        AnswerTaken::Refused(why) => {
            tracing::warn!(%why, "the workspace refused the realm");
            writeln!(out, "answered/{call}/refused 0")?;
        }
    }
    Ok(())
}
