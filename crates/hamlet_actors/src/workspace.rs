//! The `workspace` actor, one per machine: it runs the agents'
//! commands and file changes under the sandbox, within each agent's
//! reach.

pub mod reach;
mod run;

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory};
use hamlet_host::{Address, Caller, Cause, Host, HostRef};
use hamlet_messages::realm::{SecretList, Secrets};
use hamlet_messages::workspace::reach::SecretsVersion;
use hamlet_messages::workspace::{
    AgentNumber, CallRefusal, CallerRefusal, ReachAnswer,
    ReachRefusal, Run, RunAnswer, SetReach,
};
use hamlet_sandbox::{AllLeftovers, Sandbox};

use self::reach::{Gate, Op, Purpose, RealmFolder};

pub const KIND: &str = "workspace";

/// The workspace's key.
pub const MACHINE: &str = "local";

/// The kind of the realm, the only caller that sets a reach.
pub const REALM: &str = "realm";

/// The realm's key.
pub const REALM_KEY: &str = "local";

/// The kind of an agent, keyed by its [`AgentNumber`].
pub const AGENT: &str = "agent";

/// The workspace's kind. Its host gives it the [`Sandbox`] its
/// commands run in, as a value of [`hamlet_host::HostOptions`].
pub fn factory() -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<Workspace>(
        KIND,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry
        .into_factories()
        .remove(KIND)
        .expect("the registry holds the kind it registered")
}

struct Workspace {
    home: RealmFolder,
    sandbox: Sandbox,
    /// The realm's secrets, held in memory only (W3): none after a
    /// start, so the first command fetches them.
    secrets: Mutex<Option<Held>>,
    /// What commands left running under Seatbelt, ended with the
    /// workspace.
    leftovers: AllLeftovers,
}

/// A list of the realm's secrets and its version.
struct Held {
    version: SecretsVersion,
    values: Arc<Vec<String>>,
}

#[async_trait::async_trait]
impl Actor for Workspace {
    type Input = ();
    type Actions = (SetReach, Run);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        let host = HostRef::of(ctx.inner())?;
        let sandbox = *host
            .value::<Sandbox>()
            .context("the host gave the workspace no sandbox")?;
        ctx.sql()
            .exec(format!("{};{}", reach::SCHEMA, run::SCHEMA))
            .await?;
        Ok(Self {
            home: RealmFolder::new(host.folder())?,
            sandbox,
            secrets: Mutex::new(None),
            leftovers: AllLeftovers::default(),
        })
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        self.leftovers.end(|_| true);
    }
}

impl Workspace {
    /// The version of the secrets held, and the list.
    fn held(&self) -> (Option<SecretsVersion>, Arc<Vec<String>>) {
        let held = self
            .secrets
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        held.as_ref().map_or_else(
            || (None, Arc::default()),
            |held| (Some(held.version), held.values.clone()),
        )
    }

    /// Calls the realm for its secrets, and holds them when they are
    /// newer than the ones held.
    async fn fetch_secrets(&self, ctx: &Ctx<Self>) -> Result<()> {
        let answer = Host::call_from(
            ctx.inner(),
            &Address::new(REALM, REALM_KEY),
            Secrets::NAME,
            hamlet_actor::action::encode_positional(&Secrets)?,
            Duration::from_secs(10),
            Cause::root("workspace"),
        )
        .await
        .context("fetch the realm's secrets")?;
        let list: SecretList =
            ciborium::from_reader(answer.as_slice())
                .context("decode the realm's secrets")?;
        let mut held = self
            .secrets
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if held
            .as_ref()
            .is_none_or(|held| held.version < list.version)
        {
            *held = Some(Held {
                version: list.version,
                values: Arc::new(list.values),
            });
        }
        Ok(())
    }
}

/// What the gate let a command do, once its row is open.
struct Opened {
    command: hamlet_messages::workspace::CommandId,
    placement: reach::Placement,
    secrets: Arc<Vec<String>>,
}

impl Handler<Run> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        run: Run,
    ) -> Result<RunAnswer> {
        let caller = caller(&ctx)?;
        let agent = match agent_of(caller.as_ref(), run.agent) {
            Ok(agent) => agent,
            Err(refusal) => {
                refused(caller.as_ref(), Run::NAME, &refusal);
                return Ok(RunAnswer::Refused(CallRefusal::Caller(
                    refusal,
                )));
            }
        };
        if let Err(why) = checked(&run) {
            return Ok(RunAnswer::Refused(CallRefusal::Invalid(why)));
        }
        let request_hash = run::request_hash(&run)?;
        let mut fetched = false;
        let opened = loop {
            let tx = ctx.sql().begin_transaction(None).await?;
            let decided =
                self.decide(&tx, agent, &run, &request_hash).await;
            match decided {
                Ok(Decision::Open(opened)) => {
                    tx.commit().await?;
                    break opened;
                }
                Ok(Decision::Answer(answer)) => {
                    tx.rollback().await?;
                    return Ok(answer);
                }
                Ok(Decision::Fetch(version)) => {
                    tx.rollback().await?;
                    if fetched {
                        bail!(
                            "the realm's secrets are older than \
                             version {}",
                            version.0
                        );
                    }
                    self.fetch_secrets(&ctx).await?;
                    fetched = true;
                }
                Err(error) => {
                    tx.rollback().await?;
                    return Err(error);
                }
            }
        };
        let timeout = run
            .timeout_ms
            .map_or(run::LONGEST, Duration::from_millis)
            .min(run::LONGEST);
        let ended = run::start(
            run::Spec {
                command: opened.command,
                script: run.script,
                cwd: opened.placement.cwd,
                policy: opened.placement.policy,
                sandbox: self.sandbox,
                timeout,
                secrets: opened.secrets,
                leftovers: self.leftovers.of(i64::try_from(agent.0)?),
            },
            ctx.abort_signal(),
        )?;
        let end = ended.await.context("the command was cut off")?;
        let tx = ctx.sql().begin_transaction(None).await?;
        run::end(&tx, &end).await?;
        tx.commit().await?;
        Ok(RunAnswer::Done(end))
    }
}

/// What a [`Run`]'s transaction comes to.
enum Decision {
    Open(Opened),
    Answer(RunAnswer),
    /// The secrets held are older than this version.
    Fetch(SecretsVersion),
}

impl Workspace {
    /// Answers a repeat of `run` from its row, or passes it through
    /// the gate and opens its row, in `tx`.
    async fn decide(
        &self,
        tx: &hamlet_actor_core::SqliteTransaction,
        agent: AgentNumber,
        run: &Run,
        request_hash: &[u8],
    ) -> Result<Decision> {
        if let Some(row) = run::find(tx, agent, &run.call).await? {
            if row.request_hash != request_hash {
                return Ok(Decision::Answer(RunAnswer::Refused(
                    CallRefusal::CallReused(run.call.clone()),
                )));
            }
            return Ok(Decision::Answer(answer_of(row)));
        }
        let reach = reach::read(tx, agent).await?;
        let (held, secrets) = self.held();
        let op = Op::Command {
            conversation: &run.conversation,
            script: &run.script,
            network: run.network,
        };
        let network = match reach::gate(
            &self.home,
            agent,
            reach.as_ref(),
            held,
            &op,
        ) {
            Gate::Run { network } => network,
            Gate::Refuse(refusal) => {
                return Ok(Decision::Answer(RunAnswer::Refused(
                    CallRefusal::Reach(refusal),
                )));
            }
            Gate::NeedsChief(needs) => {
                return Ok(Decision::Answer(RunAnswer::Refused(
                    CallRefusal::NeedsChief(needs),
                )));
            }
            Gate::NeedsSecrets(version) => {
                return Ok(Decision::Fetch(version));
            }
        };
        let placement = reach
            .as_ref()
            .and_then(|reach| {
                reach::placement(
                    &self.home,
                    agent,
                    reach,
                    Purpose::Command { network },
                )
            })
            .context("the gate passed a command with no reach")?;
        let command =
            run::open(tx, agent, &run.call, request_hash.to_vec())
                .await?;
        Ok(Decision::Open(Opened {
            command,
            placement,
            secrets,
        }))
    }
}

/// The answer a command's row gives.
fn answer_of(row: run::Row) -> RunAnswer {
    match row.state {
        run::State::Running => RunAnswer::Running {
            command: row.command,
        },
        run::State::Ended(end) => RunAnswer::Done(end),
        run::State::CutOff => {
            RunAnswer::Done(run::cut_off(row.command))
        }
    }
}

/// Why `run` is not a request the workspace takes, in today's words.
fn checked(run: &Run) -> Result<(), String> {
    let call = &run.call.0;
    if call.is_empty()
        || call.len() > 128
        || !call.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || b"._:-".contains(&byte)
        })
    {
        return Err(format!(
            "Invalid call id: {call:?} is not 1 to 128 of A-Z, a-z, \
             0-9, '.', '_', ':' and '-'"
        ));
    }
    if run.script.len() > 1024 * 1024 {
        return Err("Invalid command: longer than 1 MiB".to_owned());
    }
    if run.timeout_ms == Some(0) {
        return Err("Invalid timeout: must be a positive number of \
                    seconds"
            .to_owned());
    }
    Ok(())
}

impl Handler<SetReach> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        set: SetReach,
    ) -> Result<ReachAnswer> {
        let caller = caller(&ctx)?;
        if !is_the_realm(caller.as_ref()) {
            let refusal = ReachRefusal::NotTheRealm;
            refused(caller.as_ref(), SetReach::NAME, &refusal);
            return Ok(ReachAnswer::Refused(refusal));
        }
        let tx = ctx.sql().begin_transaction(None).await?;
        let in_force = reach::apply(&tx, &set).await?;
        tx.commit().await?;
        Ok(ReachAnswer::InForce(in_force))
    }
}

/// Who made the call the handler runs for, as the host stamped it.
fn caller(ctx: &Ctx<Workspace>) -> Result<Option<Caller>> {
    ctx.conn().context("the call has no connection")?.params()
}

fn is_the_realm(caller: Option<&Caller>) -> bool {
    matches!(caller, Some(Caller::Actor(from)) if from.kind == REALM)
}

/// The agent a call of an agent's tool is for: the stamped caller,
/// which must be an agent and the one the request names (W2).
pub fn agent_of(
    caller: Option<&Caller>,
    named: AgentNumber,
) -> Result<AgentNumber, CallerRefusal> {
    let Some(Caller::Actor(from)) = caller else {
        return Err(CallerRefusal::NotAnAgent);
    };
    if from.kind != AGENT {
        return Err(CallerRefusal::NotAnAgent);
    }
    let caller = from
        .key
        .parse()
        .map(AgentNumber)
        .map_err(|_| CallerRefusal::NotAnAgent)?;
    if caller == named {
        Ok(caller)
    } else {
        Err(CallerRefusal::NotTheCaller { named, caller })
    }
}

/// Logs that `caller` was refused `action`, and why. A refusal
/// leaves no row, so a caller refused again and again cannot fill
/// the workspace's file.
fn refused(
    caller: Option<&Caller>,
    action: &str,
    why: &impl std::fmt::Display,
) {
    let caller = match caller {
        Some(Caller::Actor(from)) => {
            format!("{}/{}", from.kind, from.key)
        }
        Some(Caller::Client) => "client".to_owned(),
        None => "none".to_owned(),
    };
    tracing::warn!(%caller, action, %why, "refused a call");
}

#[cfg(test)]
mod tests {
    use hamlet_host::Address;

    use super::*;

    fn actor(kind: &str, key: &str) -> Option<Caller> {
        Some(Caller::Actor(Address::new(kind, key)))
    }

    #[test]
    fn a_call_is_for_the_agent_that_made_it() {
        assert_eq!(
            agent_of(actor(AGENT, "7").as_ref(), AgentNumber(7)),
            Ok(AgentNumber(7))
        );
    }

    #[test]
    fn an_agent_that_names_another_is_refused() {
        assert_eq!(
            agent_of(actor(AGENT, "8").as_ref(), AgentNumber(7)),
            Err(CallerRefusal::NotTheCaller {
                named: AgentNumber(7),
                caller: AgentNumber(8),
            })
        );
    }

    #[test]
    fn only_an_agent_calls_for_its_tools() {
        for caller in [
            actor(REALM, "7"),
            actor(AGENT, "hand"),
            Some(Caller::Client),
            None,
        ] {
            assert_eq!(
                agent_of(caller.as_ref(), AgentNumber(7)),
                Err(CallerRefusal::NotAnAgent),
                "{caller:?}"
            );
        }
    }
}
