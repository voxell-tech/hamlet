//! A stand-in for the realm: the actor `realm/local`, which sets
//! agents' reach on the workspace by calling it as itself, and
//! answers the workspace's call for its secrets with one made-up
//! secret, `demo-secret-value`, of version 1.
//!
//! ```text
//! demo realm reach <n> <version> [project=<path>] [mode=manual|auto]
//!     [net] [worker | worker=<starter>]
//!     [trust=<conversation>:edits,commands] [secrets=<v>] [ended]
//!                    set agent <n>'s reach; manual mode by default
//! ```
//!
//! `reach` prints `reach/<n> <v>`, the version in force the workspace
//! answered with, or `reach/<n> refused: <why>`.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory};
use hamlet_actors::workspace;
use hamlet_host::{Address, Caller, Cause, Host};
use hamlet_messages::realm::{SecretList, Secrets};
use hamlet_messages::workspace::reach::{
    Footing, Mode, Project, Reach, SecretsVersion, Trust,
};
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, ReachAnswer, ReachVersion, SetReach,
};

/// The kind `realm`.
pub fn factory() -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<Realm>(
        workspace::REALM,
        ActorConfig::default(),
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

#[async_trait::async_trait]
impl Actor for Realm {
    type Input = ();
    type Actions = (Grant, Secrets);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(_: &Ctx<Self>) -> Result<Self> {
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
    let [verb, agent, version, rest @ ..] = words else {
        bail!("usage: demo realm reach <n> <version> ...");
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
